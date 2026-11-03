//! Ce que l'on tape : texte envoyé au salon ou à la conversation, et les commandes /join, /msg, /kick… Sorti de session.rs (audit du 3/11/2026, 4.6).

use super::*;

impl Session {
    /// A line typed in `buffer` ("" = the console).
    /// Like `user_input`, for a plain line that answers a message: `reply` is (msgid, nick) of it.
    /// It only counts for plain text, never for a /command.
    pub fn user_input_reply(&mut self, buffer: &str, text: &str, reply: Option<(String, String)>, now: i64) -> Vec<Out> {
        self.pending_reply = reply;
        let out = self.user_input(buffer, text, now);
        self.pending_reply = None;
        out
    }

    pub fn user_input(&mut self, buffer: &str, text: &str, now: i64) -> Vec<Out> {
        let mut out = Vec::new();
        let text = text.trim_end_matches(['\r', '\n']);
        if text.trim().is_empty() {
            return out;
        }
        self.reply_buffer = buffer.to_string();
        let Some(cmdline) = text.strip_prefix('/').filter(|r| !r.starts_with('/')) else {
            let plain = text.strip_prefix('/').map_or(text, |_| &text[1..]);
            if buffer.is_empty() {
                out.push(self.error("", "Aucun salon ici : tapez /join #salon ou /msg pseudo texte (/aide pour la liste)", now));
            } else {
                let reply = self.pending_reply.take();
                self.say_with(buffer, plain, false, reply, now, &mut out);
            }
            return out;
        };
        let (cmd, rest) = cmdline.split_once(' ').unwrap_or((cmdline, ""));
        let rest = rest.trim();
        self.command(&cmd.to_ascii_lowercase(), rest, buffer, now, &mut out);
        out
    }

    /// Sends `text` to `target` (split into several lines if too long) and echoes it.
    pub(super) fn say(&mut self, target: &str, text: &str, action: bool, now: i64, out: &mut Vec<Out>) {
        self.say_with(target, text, action, None, now, out);
    }

    /// `reply` is (msgid, nick) of the message answered. With the `message-tags` capability it goes out
    /// as the `+draft/reply` tag; without it the answer starts with "nick: " so it still reads as one.
    pub(super) fn say_with(&mut self, target: &str, text: &str, action: bool, reply: Option<(String, String)>, now: i64, out: &mut Vec<Out>) {
        let is_chan = self.is_channel(target);
        let (shown, kind) = (if is_chan { self.channel_name(target) } else { target.to_string() }, if action { LineKind::Action } else { LineKind::Msg });
        let buffer = if is_chan {
            shown
        } else if SERVICES.contains(&fold(target).as_str()) {
            String::new()
        } else {
            let fresh = !self.queries.contains_key(&fold(target));
            let q = self.query_name(target);
            if fresh {
                out.push(Out::Ui(Event::Query { buffer: q.clone() }));
            }
            q
        };
        // An answer is for a conversation, with a valid identifier; never for a service or an action.
        let reply = reply.filter(|(id, _)| valid_msgid(id) && !buffer.is_empty() && !action);
        let tagged = reply.is_some() && self.cap_enabled.contains("message-tags");
        for (i, part) in split_text(text, MAX_TEXT).into_iter().enumerate() {
            let mut part = part;
            let mut reply_id = String::new();
            if i == 0 {
                if let Some((id, nick)) = &reply {
                    if tagged {
                        reply_id = id.clone();
                    } else if !nick.is_empty() && !nick.contains(char::is_whitespace) {
                        part = format!("{nick}: {part}");
                    }
                }
            }
            let wire = if action { format!("\u{1}ACTION {part}\u{1}") } else { part.clone() };
            let mut msg = Message::new("PRIVMSG", &[target, &wire]);
            if !reply_id.is_empty() {
                msg.tags.push(("+draft/reply".into(), reply_id.clone()));
            }
            out.push(Out::Send(msg.to_string()));
            let mut shown_text = part;
            if buffer.is_empty() {
                // Console echo for services: hide what follows IDENTIFY / REGISTER / GROUP.
                let low = shown_text.to_ascii_lowercase();
                for secret in ["identify ", "id ", "register ", "group ", "ghost ", "release ", "recover "] {
                    if low.starts_with(secret) {
                        let words: Vec<&str> = shown_text.split(' ').collect();
                        shown_text = format!("{} ***", words.first().copied().unwrap_or(""));
                        break;
                    }
                }
                shown_text = format!("→ {target} : {shown_text}");
            }
            let mut echo = self.line(&buffer, kind, &self.nick.clone(), shown_text, now, false, true);
            if let Out::Ui(Event::Line { line }) = &mut echo {
                line.reply_to = reply_id;
            }
            out.push(echo);
        }
    }

    pub(super) fn command(&mut self, cmd: &str, rest: &str, buffer: &str, now: i64, out: &mut Vec<Out>) {
        let in_chan = self.is_channel(buffer);
        let first = rest.split_whitespace().next().unwrap_or("");
        match cmd {
            "join" | "j" => {
                if rest.is_empty() {
                    out.push(self.error(buffer, "Usage : /join #salon [clé]", now));
                    return;
                }
                let mut it = rest.split_whitespace();
                let chans = it.next().unwrap_or("");
                let keys: Vec<&str> = it.next().map(|k| k.split(',').collect()).unwrap_or_default();
                let list: Vec<String> = chans
                    .split(',')
                    .filter(|c| !c.is_empty())
                    .map(|c| if self.is_channel(c) { c.to_string() } else { format!("#{c}") })
                    .collect();
                for (i, c) in list.iter().enumerate() {
                    match keys.get(i) {
                        Some(k) => {
                            self.keys.insert(fold(c), k.to_string());
                            out.push(Out::Send(Message::new("JOIN", &[c, k]).to_string()));
                        }
                        None => out.push(Out::Send(Message::new("JOIN", &[c]).to_string())),
                    }
                }
            }
            "part" | "leave" | "p" => {
                let (chan, reason) = if self.is_channel(first) {
                    (first.to_string(), rest[first.len()..].trim().to_string())
                } else if in_chan {
                    (buffer.to_string(), rest.to_string())
                } else {
                    out.push(self.error(buffer, "Usage : /part [#salon] [message]", now));
                    return;
                };
                let reason = if reason.is_empty() { self.cfg.part_message.clone() } else { reason };
                out.push(Out::Send(Message::new("PART", &[&chan, &reason]).to_string()));
            }
            "msg" | "m" | "privmsg" => {
                let Some((target, text)) = rest.split_once(' ') else {
                    out.push(self.error(buffer, "Usage : /msg pseudo texte", now));
                    return;
                };
                self.say(target, text.trim(), false, now, out);
            }
            "query" | "q" => {
                if first.is_empty() {
                    out.push(self.error(buffer, "Usage : /query pseudo [texte]", now));
                    return;
                }
                let q = self.query_name(first);
                out.push(Out::Ui(Event::Query { buffer: q }));
                let text = rest[first.len()..].trim();
                if !text.is_empty() {
                    self.say(first, text, false, now, out);
                }
            }
            "me" | "action" => {
                if buffer.is_empty() {
                    out.push(self.error(buffer, "Utilisez /me dans un salon ou une conversation", now));
                } else {
                    self.say(buffer, rest, true, now, out);
                }
            }
            "notice" => {
                let Some((target, text)) = rest.split_once(' ') else {
                    out.push(self.error(buffer, "Usage : /notice cible texte", now));
                    return;
                };
                out.push(Out::Send(Message::new("NOTICE", &[target, text.trim()]).to_string()));
                let b = if self.is_channel(target) { self.channel_name(target) } else { buffer.to_string() };
                out.push(self.line(&b, LineKind::Notice, &self.nick.clone(), format!("→ {target} : {}", text.trim()), now, false, true));
            }
            "nick" | "n" => {
                if first.is_empty() {
                    out.push(self.error(buffer, "Usage : /nick nouveau_pseudo", now));
                } else {
                    out.push(Out::SendNow(Message::new("NICK", &[first]).to_string()));
                }
            }
            "quit" | "exit" | "disconnect" | "deconnecter" => {
                let reason = if rest.is_empty() { self.cfg.quit_message.clone() } else { rest.to_string() };
                self.quitting = true;
                out.push(Out::SendNow(Message::new("QUIT", &[&reason]).to_string()));
                out.push(Out::Quit);
            }
            "topic" | "sujet" => {
                let (chan, text) = if self.is_channel(first) {
                    (first.to_string(), rest[first.len()..].trim().to_string())
                } else if in_chan {
                    (buffer.to_string(), rest.to_string())
                } else {
                    out.push(self.error(buffer, "Usage : /topic [#salon] [nouveau sujet]", now));
                    return;
                };
                if text.is_empty() {
                    out.push(Out::Send(Message::new("TOPIC", &[&chan]).to_string()));
                } else {
                    out.push(Out::Send(Message::new("TOPIC", &[&chan, &text]).to_string()));
                }
            }
            "kick" | "k" => {
                if !in_chan || first.is_empty() {
                    out.push(self.error(buffer, "Usage (dans un salon) : /kick pseudo [raison]", now));
                    return;
                }
                let reason = rest[first.len()..].trim();
                out.push(Out::Send(Message::new("KICK", &[buffer, first, reason]).to_string()));
            }
            "ban" | "unban" => {
                if !in_chan || first.is_empty() {
                    out.push(self.error(buffer, "Usage (dans un salon) : /ban pseudo|masque", now));
                    return;
                }
                let mask = if first.contains('!') || first.contains('@') || first.contains('*') { first.to_string() } else { format!("{first}!*@*") };
                let m = if cmd == "ban" { "+b" } else { "-b" };
                out.push(Out::Send(Message::new("MODE", &[buffer, m, &mask]).to_string()));
            }
            "mode" => {
                let mut args: Vec<String> = rest.split_whitespace().map(String::from).collect();
                if args.is_empty() {
                    args.push(if in_chan { buffer.to_string() } else { self.nick.clone() });
                } else if in_chan && (args[0].starts_with('+') || args[0].starts_with('-')) {
                    args.insert(0, buffer.to_string());
                }
                let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                out.push(Out::Send(Message::new("MODE", &refs).to_string()));
            }
            "op" | "deop" | "voice" | "devoice" | "halfop" | "dehalfop" => {
                if !in_chan || first.is_empty() {
                    out.push(self.error(buffer, format!("Usage (dans un salon) : /{cmd} pseudo [pseudo…]"), now));
                    return;
                }
                let (sign, letter) = match cmd {
                    "op" => ('+', 'o'),
                    "deop" => ('-', 'o'),
                    "voice" => ('+', 'v'),
                    "devoice" => ('-', 'v'),
                    "halfop" => ('+', 'h'),
                    _ => ('-', 'h'),
                };
                for group in rest.split_whitespace().collect::<Vec<_>>().chunks(4) {
                    let modes: String = std::iter::once(sign).chain(group.iter().map(|_| letter)).collect();
                    let mut args = vec![buffer, modes.as_str()];
                    args.extend(group.iter().copied());
                    out.push(Out::Send(Message::new("MODE", &args).to_string()));
                }
            }
            "whois" | "wi" => {
                if first.is_empty() {
                    out.push(self.error(buffer, "Usage : /whois pseudo", now));
                } else {
                    out.push(Out::Send(Message::new("WHOIS", &[first, first]).to_string()));
                }
            }
            "whowas" => {
                if first.is_empty() {
                    out.push(self.error(buffer, "Usage : /whowas pseudo", now));
                } else {
                    out.push(Out::Send(Message::new("WHOWAS", &[first]).to_string()));
                }
            }
            "away" | "absent" => {
                if rest.is_empty() {
                    out.push(Out::Send("AWAY".into()));
                } else {
                    out.push(Out::Send(Message::new("AWAY", &[rest]).to_string()));
                }
            }
            "back" => out.push(Out::Send("AWAY".into())),
            "invite" => {
                let mut it = rest.split_whitespace();
                let Some(nick) = it.next() else {
                    out.push(self.error(buffer, "Usage : /invite pseudo [#salon]", now));
                    return;
                };
                let chan = it.next().map(str::to_string).or_else(|| in_chan.then(|| buffer.to_string()));
                match chan {
                    Some(c) => out.push(Out::Send(Message::new("INVITE", &[nick, &c]).to_string())),
                    None => out.push(self.error(buffer, "Précisez le salon : /invite pseudo #salon", now)),
                }
            }
            "list" | "liste" => {
                if rest.is_empty() {
                    out.push(Out::Send("LIST".into()));
                } else {
                    out.push(Out::Send(Message::new("LIST", &[rest]).to_string()));
                }
            }
            "names" => {
                let chan = if self.is_channel(first) { first.to_string() } else { buffer.to_string() };
                if self.is_channel(&chan) {
                    out.push(Out::Send(Message::new("NAMES", &[&chan]).to_string()));
                }
            }
            "ctcp" => {
                let mut it = rest.splitn(3, ' ');
                let (Some(t), Some(c)) = (it.next(), it.next()) else {
                    out.push(self.error(buffer, "Usage : /ctcp pseudo VERSION|PING|TIME", now));
                    return;
                };
                let args = it.next().unwrap_or("");
                let body = if args.is_empty() { c.to_ascii_uppercase() } else { format!("{} {args}", c.to_ascii_uppercase()) };
                out.push(Out::Send(Message::new("PRIVMSG", &[t, &format!("\u{1}{body}\u{1}")]).to_string()));
                out.push(self.info(buffer, format!("Requête CTCP {} envoyée à {t}", c.to_ascii_uppercase()), now));
            }
            "ns" | "nickserv" | "cs" | "chanserv" | "ms" | "memoserv" | "os" | "operserv" | "hs" | "hostserv" | "bs" | "botserv" => {
                let service = match cmd {
                    "ns" | "nickserv" => "NickServ",
                    "cs" | "chanserv" => "ChanServ",
                    "ms" | "memoserv" => "MemoServ",
                    "os" | "operserv" => "OperServ",
                    "hs" | "hostserv" => "HostServ",
                    _ => "BotServ",
                };
                if rest.is_empty() {
                    out.push(self.error(buffer, format!("Usage : /{cmd} commande…"), now));
                } else {
                    self.say(service, rest, false, now, out);
                }
            }
            "quote" | "raw" | "send" => {
                if rest.is_empty() {
                    out.push(self.error(buffer, "Usage : /quote COMMANDE paramètres", now));
                } else {
                    let b = buffer.to_string();
                    out.push(self.line(&b, LineKind::Raw, "", format!("» {rest}"), now, false, true));
                    // Raw: the line goes as typed, only its terminators are removed.
                    out.push(Out::Send(rest.chars().filter(|c| !matches!(c, '\r' | '\n' | '\0')).collect()));
                }
            }
            "help" | "aide" | "?" => {
                for l in HELP {
                    out.push(self.info(buffer, *l, now));
                }
            }
            "clear" | "effacer" => {} // handled by the page
            _ => out.push(self.error(buffer, format!("Commande inconnue : /{cmd} (tapez /aide)"), now)),
        }
    }
}

const HELP: &[&str] = &[
    "Commandes : /join #salon [clé] · /part [#salon] [message] · /msg pseudo texte · /query pseudo · /me action",
    "            /nick pseudo · /away [message] · /whois pseudo · /topic [sujet] · /invite pseudo [#salon]",
    "            /kick pseudo [raison] · /ban pseudo · /op · /deop · /voice · /devoice · /mode [cible] +/-modes",
    "            /list [filtre] · /ns /cs /ms … (services) · /ctcp pseudo VERSION · /quote COMMANDE · /clear · /quit [message]",
    "Un texte sans / est envoyé au salon ou à la conversation de la fenêtre. // au début envoie un / littéral.",
];
