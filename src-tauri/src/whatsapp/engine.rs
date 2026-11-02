//! The WhatsApp connection of one account: pairing, events in, messages out.

use super::chats::{group_buffer, user_buffer, user_of, Registry};
use crate::chat::event::{ConnState, Event as Ev, Line, LineKind, Media, PairingCode, UserEntry};
use crate::chat::{now_ms, sniff_image, CallCommand, Command, Sink, Start, Task, MAX_IMAGE_BYTES};
use base64::Engine as _;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
mod calls;

use whatsapp_rust::features::GroupParticipant;
use whatsapp_rust::prelude::*;

/// Messages we sent in this run, to recognise their echo among the incoming ones.
const MAX_SENT_IDS: usize = 500;
/// Groups listed in the main menu at connection.
const MAX_GROUPS: usize = 250;

struct Ctx {
    sink: Arc<dyn Sink>,
    registry: Mutex<Registry>,
    /// Our own name (the one we gave WhatsApp).
    me: Mutex<String>,
    /// Private chats whose window was already asked for in this run.
    announced: Mutex<HashSet<String>>,
    sent: Mutex<Vec<String>>,
    /// The device was unlinked (from the phone, or by `/logout`): the account is over.
    unlinked: AtomicBool,
    /// Calls (ringing, and the one in progress).
    calls: calls::Calls,
    /// The last messages that carry an image (by identifier), to fetch the whole image when asked.
    images: Mutex<VecDeque<(String, Arc<wa::Message>)>>,
    /// Chats of the account's history, kept until the names have arrived (see `touch`).
    pending: Mutex<Vec<Pending>>,
    /// When the last name or history arrived (ms), and whether a task waits for it to calm down.
    last_activity: AtomicI64,
    flush_planned: AtomicBool,
    /// Where this account's logs are (a chat that gets a name takes its log along).
    logs_root: PathBuf,
    server_name: String,
}

/// A chat of the history: what it is called, and its last messages.
struct Pending {
    id: String,
    group: bool,
    title: String,
    messages: Vec<wa::WebMessageInfo>,
}

/// Images whose whole content can still be fetched.
const MAX_IMAGES_KEPT: usize = 300;
/// The preview sent with an image, at most (bytes).
const MAX_THUMB: usize = 60_000;
/// Lines of history shown per chat.
const HISTORY_LINES: usize = 25;
/// Chats taken from the history of the account.
const MAX_HISTORY_CHATS: usize = 200;
/// Messages kept per chat while the names are awaited.
const KEPT_MESSAGES: usize = 60;
/// How long names and history must stay quiet before the chats are put in the menu.
const QUIET_MS: i64 = 5000;

pub fn launch(start: Start) -> Result<Task, String> {
    let Start { dir, id, entry, settings, sink, commands } = start;
    std::fs::create_dir_all(dir.join("whatsapp")).map_err(|e| format!("Dossier de la session WhatsApp : {e}"))?;
    let files = super::files(&dir, &id);
    let (db, chats) = match (files.first(), files.get(3)) {
        (Some(db), Some(chats)) => (db.clone(), chats.clone()),
        _ => return Err("Identifiant de compte invalide".into()),
    };
    let ctx = Arc::new(Ctx {
        sink,
        registry: Mutex::new(Registry::load(chats)),
        me: Mutex::new(if entry.nick.trim().is_empty() { settings.nick.clone() } else { entry.nick.clone() }),
        announced: Mutex::new(HashSet::new()),
        sent: Mutex::new(Vec::new()),
        unlinked: AtomicBool::new(false),
        calls: calls::Calls::default(),
        images: Mutex::new(VecDeque::new()),
        pending: Mutex::new(Vec::new()),
        last_activity: AtomicI64::new(0),
        flush_planned: AtomicBool::new(false),
        logs_root: dir.join("logs"),
        server_name: entry.name.clone(),
    });
    Ok(Box::pin(run(ctx, db, commands)))
}

async fn run(ctx: Arc<Ctx>, db: PathBuf, mut commands: tokio::sync::mpsc::UnboundedReceiver<Command>) {
    ctx.status(ConnState::Connecting, "Ouverture de la session WhatsApp…");
    let built = async {
        let store = SqliteStore::new(&db.to_string_lossy()).await.map_err(|e| format!("Session : {e}"))?;
        let handler_ctx = ctx.clone();
        Bot::builder()
            .with_backend(store)
            .on_event(move |ev: Arc<Event>, client: Arc<Client>| {
                let ctx = handler_ctx.clone();
                async move { ctx.on_event(&ev, &client).await }
            })
            .build()
            .await
            .map_err(|e| format!("Démarrage : {e}"))
    }
    .await;
    let bot = match built {
        Ok(bot) => bot,
        Err(e) => {
            ctx.status(ConnState::Disconnected, &e);
            return;
        }
    };
    let client = bot.client();
    let mut handle = bot.spawn();
    loop {
        tokio::select! {
            _ = &mut handle => break,
            cmd = commands.recv() => match cmd {
                None | Some(Command::Disconnect(_)) => {
                    ctx.hang_up_everything().await;
                    handle.shutdown().await;
                    ctx.registry.lock().unwrap().save();
                    ctx.status(ConnState::Disconnected, "Déconnecté");
                    return;
                }
                Some(Command::Input { buffer, text, reply }) => ctx.on_input(&client, &buffer, &text, reply).await,
                // the frames of a call keep their order: handed over at once; the rest of a call may wait for the network
                Some(Command::Call(c @ (CallCommand::Audio { .. } | CallCommand::Video { .. }))) => ctx.call_media_in(c),
                Some(Command::Call(c)) => {
                    let (ctx, client) = (ctx.clone(), client.clone());
                    tokio::spawn(async move { ctx.on_call_command(&client, c).await });
                }
                // an image can take a while to travel: the other commands do not wait for it
                Some(Command::FetchMedia { msgid, reply }) => {
                    let (ctx, client) = (ctx.clone(), client.clone());
                    tokio::spawn(async move {
                        let _ = reply.send(ctx.fetch_image(&client, &msgid).await);
                    });
                }
                Some(Command::SendImage { buffer, data, thumb, caption, reply }) => {
                    let (ctx, client) = (ctx.clone(), client.clone());
                    tokio::spawn(async move {
                        let r = ctx.send_image(&client, &buffer, data, thumb, caption).await;
                        if let Err(e) = &r {
                            ctx.line(&buffer, LineKind::Error, &format!("Image non envoyée : {e}"));
                        }
                        let _ = reply.send(r);
                    });
                }
                Some(Command::ConnectNow | Command::SetFilters { .. }) => {}
            },
        }
    }
    ctx.registry.lock().unwrap().save();
    let text = if ctx.unlinked.load(Ordering::Relaxed) { "Appareil délié de WhatsApp" } else { "Connexion terminée" };
    ctx.status(ConnState::Disconnected, text);
}

impl Ctx {
    fn status(&self, state: ConnState, text: &str) {
        self.sink.emit(Ev::Status { state, text: text.to_string() });
    }

    fn line(&self, buffer: &str, kind: LineKind, text: &str) {
        self.sink.emit(Ev::Line {
            line: Line { buffer: buffer.to_string(), kind, nick: String::new(), text: text.to_string(), time: now_ms(), highlight: false, own: false, msgid: String::new(), reply_to: String::new(), media: None },
        });
    }

    async fn on_event(self: &Arc<Self>, ev: &Event, client: &Arc<Client>) {
        match ev {
            Event::PairingQrCode(q) => {
                if let Some((size, modules)) = qr_modules(&q.code) {
                    self.sink.emit(Ev::Pairing { code: PairingCode { size, modules, seconds: q.timeout.as_secs() as u32 } });
                    self.status(ConnState::Registering, "Jumelage : scannez le code avec WhatsApp (Réglages > Appareils connectés)");
                }
            }
            Event::PairSuccess(_) => {
                self.sink.emit(Ev::Paired);
                self.status(ConnState::Registering, "Jumelage réussi, connexion…");
            }
            Event::Connected(_) => {
                self.sink.emit(Ev::Paired);
                let name = client.push_name();
                if !name.is_empty() {
                    *self.me.lock().unwrap() = name;
                }
                let me = self.me.lock().unwrap().clone();
                self.sink.emit(Ev::Me { nick: me, network: "WhatsApp".into() });
                self.status(ConnState::Connected, "Connecté à WhatsApp");
                self.list_groups(client).await;
                self.heal_names(client).await;
                self.list_private_chats();
            }
            Event::Disconnected(_) => self.status(ConnState::Reconnecting, "Connexion perdue : nouvelle tentative"),
            Event::LoggedOut(_) => {
                self.unlinked.store(true, Ordering::Relaxed);
                self.sink.emit(Ev::Paired);
                self.line("", LineKind::Error, "WhatsApp a délié cet appareil : liez-le de nouveau pour continuer.");
            }
            Event::IncomingCall(c) => self.on_incoming_call(client, c).await,
            Event::MissedCall(m) => self.end_incoming(&m.call_id, "Appel manqué"),
            Event::CallEndedElsewhere(e) => self.end_incoming(&e.call_id, "Appel pris sur un autre appareil"),
            Event::ClientOutdated(_) => self.line("", LineKind::Error, "WhatsApp refuse cette version de la bibliothèque : mettez Messages à jour."),
            Event::TemporaryBan(_) => self.line("", LineKind::Error, "WhatsApp a suspendu temporairement ce compte."),
            Event::PushNameUpdate(u) => self.registry.lock().unwrap().set_name(&u.jid.to_string(), &u.new_push_name),
            // the names saved in the phone's address book
            Event::ContactUpdate(u) => {
                let a = &u.action;
                let name = a.full_name.as_deref().filter(|n| !n.trim().is_empty()).or(a.first_name.as_deref());
                let mut r = self.registry.lock().unwrap();
                let jid = u.jid.to_string();
                for other in [a.lid_jid.as_deref(), a.pn_jid.as_deref()].into_iter().flatten() {
                    r.link(&jid, other);
                }
                if let Some(n) = name {
                    r.set_name(&jid, n);
                }
                drop(r);
                self.touch(client);
            }
            // the chats and the names the account already had on the phone
            Event::HistorySync(h) => {
                if let Some(hs) = h.get() {
                    self.on_history(hs);
                    self.touch(client);
                }
            }
            Event::Messages(_) => {
                for m in ev.messages() {
                    self.on_message(client, &m.message, &m.info).await;
                }
                self.registry.lock().unwrap().save();
            }
            _ => {}
        }
    }

    /// The private chats known from earlier sessions appear in the main menu again (the state starts empty at each run).
    fn list_private_chats(&self) {
        let chats = self.registry.lock().unwrap().private_buffers();
        for buffer in chats {
            self.sink.emit(Ev::Listed { buffer });
        }
    }

    /// The groups the account belongs to appear in the main menu, with their members.
    async fn list_groups(&self, client: &Arc<Client>) {
        let groups = match client.groups().get_participating().await {
            Ok(g) => g,
            Err(e) => {
                self.line("", LineKind::Error, &format!("Liste des groupes : {e}"));
                return;
            }
        };
        for (jid, meta) in groups.iter().take(MAX_GROUPS) {
            self.register_group(&jid.to_string(), &meta.subject, &meta.participants);
        }
        self.registry.lock().unwrap().save();
    }

    /// Names or history just arrived: once they stop arriving, the chats are put in the menu with their names.
    fn touch(self: &Arc<Self>, client: &Arc<Client>) {
        self.last_activity.store(now_ms(), Ordering::Relaxed);
        if self.flush_planned.swap(true, Ordering::Relaxed) {
            return;
        }
        let (this, client) = (self.clone(), client.clone());
        tokio::spawn(async move {
            while now_ms() - this.last_activity.load(Ordering::Relaxed) < QUIET_MS {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            this.flush_planned.store(false, Ordering::Relaxed);
            this.flush_history(&client).await;
            this.list_groups(&client).await;
            this.heal_names(&client).await;
        });
    }

    /// What the account already had that matters at once: names, and the chats kept for later.
    fn on_history(&self, hs: &wa::HistorySync) {
        let mut r = self.registry.lock().unwrap();
        for p in &hs.phone_number_to_lid_mappings {
            if let (Some(pn), Some(lid)) = (p.pn_jid.as_deref(), p.lid_jid.as_deref()) {
                r.link(pn, lid);
            }
        }
        for p in &hs.pushnames {
            if let (Some(id), Some(name)) = (p.id.as_deref(), p.pushname.as_deref()) {
                r.set_name(id, name);
            }
        }
        let mut pending = self.pending.lock().unwrap();
        for c in hs.conversations.iter().take(MAX_HISTORY_CHATS) {
            let id = c.id.as_str();
            if id.is_empty() || id.contains("@newsletter") || id.contains("@broadcast") {
                continue;
            }
            let group = id.ends_with("@g.us");
            let title = c.name.as_deref().or(c.display_name.as_deref()).map(str::trim).unwrap_or("").to_string();
            if !group {
                if let Some(pn) = c.pn_jid.as_deref() {
                    r.link(id, pn);
                }
                if !title.is_empty() {
                    r.set_name(id, &title);
                }
            }
            let mut messages: Vec<wa::WebMessageInfo> = c.messages.iter().filter_map(|m| m.message.as_option().cloned()).collect();
            messages.sort_by_key(|m| m.message_timestamp.unwrap_or(0));
            let skip = messages.len().saturating_sub(KEPT_MESSAGES);
            messages.drain(..skip);
            pending.retain(|p| p.id != id);
            if pending.len() < MAX_HISTORY_CHATS {
                pending.push(Pending { id: id.to_string(), group, title, messages });
            }
        }
    }

    /// The phone number behind an anonymous identifier, from what the library learned: its names become known.
    async fn resolve_lid(&self, client: &Arc<Client>, jid: &str) {
        if !jid.ends_with("@lid") {
            return;
        }
        let Ok(parsed) = jid.parse::<Jid>() else { return };
        if let Ok(Some(entry)) = client.get_lid_pn_entry(&parsed).await {
            self.registry.lock().unwrap().link(jid, &format!("{}@s.whatsapp.net", entry.phone_number));
        }
    }

    /// The chats of the history go in the menu, called by their names, with their last lines in the log.
    async fn flush_history(&self, client: &Arc<Client>) {
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        for p in &pending {
            self.resolve_lid(client, &p.id).await;
        }
        let me = self.me.lock().unwrap().clone();
        let mut listed: Vec<String> = Vec::new();
        let mut batches: Vec<(String, Vec<Line>)> = Vec::new();
        {
            let mut r = self.registry.lock().unwrap();
            for p in &pending {
                let wanted = if p.group {
                    group_buffer(if p.title.is_empty() { user_of(&p.id) } else { &p.title })
                } else {
                    user_buffer(&r.display(&p.id), user_of(&p.id))
                };
                let buffer = r.buffer_for(&p.id, p.group, &wanted);
                for w in &p.messages {
                    if let (Some(m), Some(id)) = (w.message.as_option(), w.key.as_option().and_then(|k| k.id.as_deref())) {
                        self.remember(id, m);
                    }
                }
                let mut lines: Vec<Line> = p.messages.iter().filter_map(|w| history_line(&mut r, &me, &buffer, &p.id, p.group, w)).collect();
                lines.sort_by_key(|l| l.time);
                let skip = lines.len().saturating_sub(HISTORY_LINES);
                lines.drain(..skip);
                if !lines.is_empty() {
                    batches.push((buffer.clone(), lines));
                }
                listed.push(buffer);
            }
            r.save();
        }
        for buffer in listed {
            self.sink.emit(Ev::Listed { buffer });
        }
        for (buffer, lines) in batches {
            self.sink.emit(Ev::Backlog { buffer, lines });
        }
    }

    /// Chats registered under a number, whose name is known by now, take it (and their log with them).
    async fn heal_names(&self, client: &Arc<Client>) {
        let unnamed = self.registry.lock().unwrap().unnamed_private();
        for jid in unnamed {
            self.resolve_lid(client, &jid).await;
            let renamed = {
                let mut r = self.registry.lock().unwrap();
                let wanted = user_buffer(&r.display(&jid), user_of(&jid));
                r.rename(&jid, &wanted)
            };
            if let Some((old, new)) = renamed {
                crate::logs::rename_buffer(&self.logs_root, &self.server_name, &old, &new);
                self.sink.emit(Ev::Listed { buffer: new });
            }
        }
        self.registry.lock().unwrap().save();
    }

    /// A group joins the main menu, with its members (admins marked like operators).
    fn register_group(&self, jid: &str, subject: &str, participants: &[GroupParticipant]) -> String {
        let (buffer, users) = {
            let mut r = self.registry.lock().unwrap();
            let wanted = if subject.trim().is_empty() { group_buffer(user_of(jid)) } else { group_buffer(subject) };
            let buffer = r.buffer_for(jid, true, &wanted);
            let users: Vec<UserEntry> = participants
                .iter()
                .map(|p| {
                    let id = p.jid.to_string();
                    let phone = p.phone_number.as_ref().map(ToString::to_string);
                    let name = r
                        .name_of(&id)
                        .or_else(|| phone.as_deref().and_then(|n| r.name_of(n)))
                        .map_or_else(|| user_buffer("", user_of(phone.as_deref().unwrap_or(&id))), str::to_string);
                    UserEntry { nick: name, modes: if p.is_admin() { "@".into() } else { String::new() }, away: false }
                })
                .collect();
            (buffer, users)
        };
        self.sink.emit(Ev::Listed { buffer: buffer.clone() });
        self.sink.emit(Ev::Users { buffer: buffer.clone(), users });
        buffer
    }

    /// The buffer of the chat a message belongs to (a group not seen yet is looked up first).
    async fn buffer_of_chat(&self, client: &Arc<Client>, chat: &str, group: bool, jid: &Jid) -> String {
        if let Some(b) = self.registry.lock().unwrap().buffer_of(chat) {
            return b.to_string();
        }
        if group {
            return match client.groups().get_metadata(jid).await {
                Ok(meta) => self.register_group(chat, &meta.subject, &meta.participants),
                Err(_) => self.register_group(chat, "", &[]),
            };
        }
        self.resolve_lid(client, chat).await;
        let mut r = self.registry.lock().unwrap();
        let wanted = user_buffer(&r.display(chat), user_of(chat));
        r.buffer_for(chat, false, &wanted)
    }

    async fn on_message(&self, client: &Arc<Client>, msg: &wa::Message, info: &MessageInfo) {
        let chat = info.source.chat.to_string();
        if chat.contains("@newsletter") || chat.contains("@broadcast") {
            return;
        }
        let Some((kind, text)) = describe(msg) else { return };
        let own = info.source.is_from_me;
        let id = info.id.to_string();
        if own && self.sent.lock().unwrap().contains(&id) {
            return; // the echo of what we just sent
        }
        let group = info.source.is_group || chat.ends_with("@g.us");
        let sender = info.source.sender.to_string();
        if !own && !info.push_name.is_empty() {
            self.registry.lock().unwrap().set_name(&sender, &info.push_name);
        }
        self.remember(&id, msg);
        let buffer = self.buffer_of_chat(client, &chat, group, &info.source.chat).await;
        if !group && self.announced.lock().unwrap().insert(buffer.clone()) {
            self.sink.emit(Ev::Query { buffer: buffer.clone() });
        }
        let me = self.me.lock().unwrap().clone();
        let nick = if own { me.clone() } else { self.registry.lock().unwrap().display(&sender) };
        let first = me.split_whitespace().next().unwrap_or("");
        let highlight = !own && (!group || (!first.is_empty() && crate::irc::session::mentions(&text, first)));
        self.sink.emit(Ev::Line {
            line: Line { buffer, kind, nick, text, time: info.timestamp.timestamp_millis(), highlight, own, msgid: id, reply_to: replied_to(msg).unwrap_or_default(), media: image_media(msg) },
        });
    }

    async fn on_input(&self, client: &Arc<Client>, buffer: &str, text: &str, reply: Option<(String, String)>) {
        let text = text.trim_end_matches(['\r', '\n']);
        if text.trim().is_empty() {
            return;
        }
        if let Some(cmd) = text.strip_prefix('/').filter(|r| !r.starts_with('/')) {
            let (name, rest) = cmd.split_once(' ').map_or((cmd, ""), |(a, b)| (a, b.trim()));
            match name.to_ascii_lowercase().as_str() {
                "query" | "msg" => self.open_chat(rest),
                "logout" => {
                    self.line(buffer, LineKind::Info, "Déliement de l'appareil de WhatsApp…");
                    client.logout().await;
                }
                "aide" | "help" => {
                    for l in [
                        "/query numéro ou nom : ouvre une conversation (le numéro avec l'indicatif, sans « + » : 33612345678)",
                        "/logout : délie cet appareil de votre compte WhatsApp",
                        "Les groupes sont dans le menu principal ; écrivez dans leur fenêtre.",
                    ] {
                        self.line(buffer, LineKind::Info, l);
                    }
                }
                other => self.line(buffer, LineKind::Error, &format!("Commande inconnue : /{other} (/aide pour la liste)")),
            }
            return;
        }
        let plain = text.strip_prefix('/').map_or(text, |_| &text[1..]);
        // an answer: WhatsApp's quote needs the original message, which is not kept here; the name tells whom it is for
        let plain = match reply {
            Some((_, nick)) if !nick.is_empty() => format!("{nick} : {plain}"),
            _ => plain.to_string(),
        };
        if buffer.is_empty() {
            self.line("", LineKind::Error, "Aucune conversation ici : /query numéro, ou ouvrez un groupe depuis le menu principal.");
            return;
        }
        let target = self.registry.lock().unwrap().chat_of(buffer).map(|(j, _)| j.to_string());
        let Some(jid) = target.and_then(|j| j.parse::<Jid>().ok()) else {
            self.line(buffer, LineKind::Error, "Conversation inconnue de WhatsApp.");
            return;
        };
        match client.send_message(&jid, wa::Message::text(plain.as_str())).await {
            Ok(sent) => {
                {
                    let mut ids = self.sent.lock().unwrap();
                    if ids.len() >= MAX_SENT_IDS {
                        ids.remove(0);
                    }
                    ids.push(sent.message_id.clone());
                }
                let me = self.me.lock().unwrap().clone();
                self.sink.emit(Ev::Line {
                    line: Line { buffer: buffer.to_string(), kind: LineKind::Msg, nick: me, text: plain, time: now_ms(), highlight: false, own: true, msgid: sent.message_id, reply_to: String::new(), media: None },
                });
            }
            Err(e) => self.line(buffer, LineKind::Error, &format!("Message non envoyé : {e}")),
        }
    }

    /// Keeps a message that carries an image, so that the whole image can be fetched later.
    fn remember(&self, id: &str, msg: &wa::Message) {
        if id.is_empty() || msg.image_message.as_option().is_none() {
            return;
        }
        let mut images = self.images.lock().unwrap();
        images.retain(|(i, _)| i != id);
        if images.len() >= MAX_IMAGES_KEPT {
            images.pop_front();
        }
        images.push_back((id.to_string(), Arc::new(msg.clone())));
    }

    /// The whole image of a message received (or sent) in this session.
    async fn fetch_image(&self, client: &Arc<Client>, msgid: &str) -> Result<Vec<u8>, String> {
        let msg = self.images.lock().unwrap().iter().find(|(i, _)| i == msgid).map(|(_, m)| m.clone());
        let msg = msg.ok_or("Image introuvable : elle date d'avant cette session de Messages")?;
        let img = msg.image_message.as_option().ok_or("Ce message n'est pas une image")?;
        if usize::try_from(img.file_length.unwrap_or(0)).unwrap_or(usize::MAX) > MAX_IMAGE_BYTES {
            return Err("Image trop grosse (10 Mo au plus)".into());
        }
        let bytes = client.download(img).await.map_err(|e| format!("Téléchargement : {e}"))?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("Image trop grosse (10 Mo au plus)".into());
        }
        sniff_image(&bytes).ok_or("Format d'image non pris en charge")?;
        Ok(bytes)
    }

    /// Sends an image to a chat; its line appears like a message of ours.
    async fn send_image(&self, client: &Arc<Client>, buffer: &str, data: Vec<u8>, thumb: Vec<u8>, caption: String) -> Result<(), String> {
        let mime = sniff_image(&data).ok_or("Format non pris en charge (JPEG, PNG ou WebP seulement)")?;
        if data.len() > MAX_IMAGE_BYTES {
            return Err("Image trop grosse (10 Mo au plus)".into());
        }
        let target = self.registry.lock().unwrap().chat_of(buffer).map(|(j, _)| j.to_string());
        let jid = target.and_then(|j| j.parse::<Jid>().ok()).ok_or("Conversation inconnue de WhatsApp")?;
        let thumb = (!thumb.is_empty() && thumb.len() <= MAX_THUMB && sniff_image(&thumb) == Some("image/jpeg")).then_some(thumb);
        let upload = client.upload(data, whatsapp_rust::download::MediaType::Image, whatsapp_rust::upload::UploadOptions::new()).await.map_err(|e| format!("Envoi : {e}"))?;
        let caption = caption.trim().to_string();
        let msg = whatsapp_rust::media::image_message(
            upload,
            whatsapp_rust::media::ImageOptions { caption: (!caption.is_empty()).then(|| caption.clone()), mimetype: Some(mime.to_string()), jpeg_thumbnail: thumb, context_info: None },
        );
        let sent = client.send_message(&jid, msg.clone()).await.map_err(|e| format!("Envoi : {e}"))?;
        {
            let mut ids = self.sent.lock().unwrap();
            if ids.len() >= MAX_SENT_IDS {
                ids.remove(0);
            }
            ids.push(sent.message_id.clone());
        }
        self.remember(&sent.message_id, &msg);
        let me = self.me.lock().unwrap().clone();
        let text = if caption.is_empty() { "[image]".to_string() } else { format!("[image] {caption}") };
        self.sink.emit(Ev::Line {
            line: Line { buffer: buffer.to_string(), kind: LineKind::Msg, nick: me, text, time: now_ms(), highlight: false, own: true, msgid: sent.message_id, reply_to: String::new(), media: image_media(&msg) },
        });
        Ok(())
    }

    /// `/query x`: x is a name already known, or a number with its country code.
    fn open_chat(&self, wanted: &str) {
        if wanted.is_empty() {
            self.line("", LineKind::Error, "Usage : /query numéro (avec l'indicatif, par exemple 33612345678) ou nom");
            return;
        }
        let buffer = {
            let mut r = self.registry.lock().unwrap();
            let jid = r.find_user(wanted).map(str::to_string).or_else(|| number_jid(wanted));
            jid.map(|j| {
                let wanted = user_buffer(&r.display(&j), user_of(&j));
                r.buffer_for(&j, false, &wanted)
            })
        };
        match buffer {
            Some(b) => {
                self.announced.lock().unwrap().insert(b.clone());
                self.sink.emit(Ev::Query { buffer: b });
            }
            None => self.line("", LineKind::Error, "Contact inconnu : écrivez son numéro avec l'indicatif (par exemple /query 33612345678)."),
        }
    }
}

/// One message of the account's history, as a line of its chat (None for what is not a message to show).
fn history_line(r: &mut Registry, me: &str, buffer: &str, chat: &str, group: bool, w: &wa::WebMessageInfo) -> Option<Line> {
    let msg = w.message.as_option()?;
    let (kind, text) = describe(msg)?;
    let key = w.key.as_option();
    let own = key.and_then(|k| k.from_me).unwrap_or(false);
    let sender = if group { w.participant.clone().or_else(|| key.and_then(|k| k.participant.clone())).unwrap_or_default() } else { chat.to_string() };
    if !own && !sender.is_empty() {
        if let Some(pn) = w.push_name.as_deref() {
            r.set_name(&sender, pn);
        }
    }
    let nick = if own { me.to_string() } else if sender.is_empty() { String::new() } else { r.display(&sender) };
    Some(Line {
        buffer: buffer.to_string(),
        kind,
        nick,
        text,
        time: i64::try_from(w.message_timestamp.unwrap_or(0)).unwrap_or(0).saturating_mul(1000),
        highlight: false,
        own,
        msgid: key.and_then(|k| k.id.clone()).unwrap_or_default(),
        reply_to: replied_to(msg).unwrap_or_default(),
        media: image_media(msg),
    })
}

/// The identifier of a phone number typed with its country code ("33612345678", "+33 6 12 34 56 78").
fn number_jid(typed: &str) -> Option<String> {
    let t = typed.trim().trim_start_matches('+');
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || c == ' ') {
        return None;
    }
    let digits: String = t.chars().filter(char::is_ascii_digit).collect();
    (6..=15).contains(&digits.len()).then(|| format!("{digits}@s.whatsapp.net"))
}

/// What a message says, for a line: the text, a caption, or a word for what was sent.
fn describe(m: &wa::Message) -> Option<(LineKind, String)> {
    if let Some(t) = m.text_content() {
        return Some((LineKind::Msg, t.to_string()));
    }
    let caption = m.get_caption().map(str::to_string).filter(|c| !c.is_empty());
    let with = |what: &str| Some((LineKind::Msg, caption.as_deref().map_or_else(|| format!("[{what}]"), |c| format!("[{what}] {c}"))));
    if m.image_message.as_option().is_some() {
        return with("image");
    }
    if m.video_message.as_option().is_some() {
        return with("vidéo");
    }
    if m.audio_message.as_option().is_some() {
        return with("message vocal");
    }
    if let Some(d) = m.document_message.as_option() {
        return with(&format!("document : {}", d.file_name.as_deref().unwrap_or("sans nom")));
    }
    if m.sticker_message.as_option().is_some() {
        return with("autocollant");
    }
    if m.location_message.as_option().is_some() {
        return with("position");
    }
    if m.contact_message.as_option().is_some() {
        return with("contact");
    }
    if let Some(r) = m.reaction_message.as_option() {
        let emoji = r.text.as_deref().unwrap_or("");
        return (!emoji.is_empty()).then(|| (LineKind::Action, format!("réagit {emoji}")));
    }
    None
}

/// What an image message shows at once: its small preview (possibly none: the whole image is still fetchable).
fn image_media(m: &wa::Message) -> Option<Media> {
    let img = m.image_message.as_option()?;
    let thumb = img.jpeg_thumbnail.as_deref().filter(|t| !t.is_empty() && t.len() <= MAX_THUMB && sniff_image(t) == Some("image/jpeg"));
    Some(Media {
        kind: "image".into(),
        thumb: thumb.map(|t| base64::engine::general_purpose::STANDARD.encode(t)).unwrap_or_default(),
        width: img.width.unwrap_or(0).min(20_000),
        height: img.height.unwrap_or(0).min(20_000),
    })
}

/// The identifier of the message that this one answers.
fn replied_to(m: &wa::Message) -> Option<String> {
    m.extended_text_message.as_option()?.context_info.as_option()?.stanza_id.clone()
}

/// The squares of a QR code, row by row: '1' dark, '0' light.
fn qr_modules(payload: &str) -> Option<(u16, String)> {
    let qr = qrcode::QrCode::new(payload.as_bytes()).ok()?;
    let size = u16::try_from(qr.width()).ok()?;
    let modules = qr.to_colors().iter().map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' }).collect();
    Some((size, modules))
}

#[cfg(test)]
mod tests {
    use super::{number_jid, qr_modules};

    #[test]
    fn a_qr_code_is_a_square_of_modules() {
        let (size, modules) = qr_modules("2@abc,def,ghi,1").unwrap();
        assert!(size >= 21);
        assert_eq!(modules.len(), usize::from(size) * usize::from(size));
        assert!(modules.chars().all(|c| c == '0' || c == '1'));
        assert!(modules.contains('1') && modules.contains('0'));
    }

    #[test]
    fn a_number_becomes_an_identifier_and_a_name_does_not() {
        assert_eq!(number_jid("33612345678").as_deref(), Some("33612345678@s.whatsapp.net"));
        assert_eq!(number_jid("+33 6 12 34 56 78").as_deref(), Some("33612345678@s.whatsapp.net"));
        assert_eq!(number_jid("Alice"), None);
        assert_eq!(number_jid("123"), None);
        assert_eq!(number_jid(""), None);
        assert_eq!(number_jid("33612345678@evil"), None);
    }
}
