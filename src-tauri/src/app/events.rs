//! Les événements d'un serveur (lignes, état, notifications) envoyés aux fenêtres qui les veulent. Sorti de app.rs (audit du 3/11/2026, 4.6).

use super::*;

#[derive(Serialize, Clone)]
struct Payload<'a> {
    server: &'a str,
    seq: u64,
    event: &'a Event,
}

pub struct AppSink {
    pub app: AppHandle,
    pub server: String,
}

impl Sink for AppSink {
    fn emit(&self, event: Event) {
        handle_event(&self.app, &self.server, event);
    }

    fn media(&self, call: &str, kind: &'static str, data: &[u8]) {
        use base64::Engine as _;
        #[derive(Serialize, Clone)]
        struct Media<'a> {
            server: &'a str,
            call: &'a str,
            kind: &'static str,
            data: String,
        }
        let data = base64::engine::general_purpose::STANDARD.encode(data);
        let _ = self.app.emit("call-media", Media { server: &self.server, call, kind, data });
    }
}

/// A line made by the application itself (the answer to a command it handles), shown in a buffer.
pub fn local_line(app: &AppHandle, server: &str, buffer: &str, kind: LineKind, text: &str) {
    let time = chat::now_ms();
    let line = Line { buffer: buffer.to_string(), kind, nick: String::new(), text: text.to_string(), time, highlight: false, own: false, msgid: String::new(), reply_to: String::new(), media: None };
    handle_event(app, server, Event::Line { line });
}

pub(super) fn handle_event(app: &AppHandle, server: &str, ev: Event) {
    let st = app.state::<App>();
    let (open_queries, notifications, log_enabled, server_name, hidden) = {
        let c = st.config.lock().unwrap();
        let hidden = matches!(&ev, Event::Line { line } if c.is_hidden(server, &line.buffer));
        (c.settings.open_queries, c.settings.notifications, c.settings.log_enabled, c.server(server).map(|e| e.name.clone()), hidden)
    };
    // Is somebody looking at the window of the buffer a line belongs to?
    // Asked before taking any lock: `is_focused` waits for the main thread, which may itself be
    // waiting for the state lock (a window gaining focus marks its buffer as read).
    // Older lines from the account's history: into the log only, so no notification and no unread counter.
    if let Event::Backlog { lines, .. } = &ev {
        if log_enabled {
            if let Some(name) = &server_name {
                for line in lines {
                    st.logger.log(name, line);
                }
            }
        }
        return;
    }
    let watched = match &ev {
        Event::Line { line } if !line.buffer.is_empty() => app
            .get_webview_window(&Kind::Chat(server.to_string(), line.buffer.clone()).label())
            .is_some_and(|w| w.is_focused().unwrap_or(false)),
        _ => false,
    };
    let seq = {
        let mut states = st.states.lock().unwrap();
        // A server deleted while its connection winds down must not come back as a ghost.
        let Some(s) = states.get_mut(server) else { return };
        s.apply(&ev);
        if let Event::Line { line } = &ev {
            if !line.own && !line.buffer.is_empty() && matches!(line.kind, LineKind::Msg | LineKind::Action | LineKind::Notice) && !watched {
                s.bump(&line.buffer, line.highlight);
            }
        }
        s.seq
    };

    // What the pages are told: a channel list stays here, they only learn that it is ready.
    let shown = match &ev {
        Event::ChannelList { entries } => Event::ListReady { count: entries.len() },
        other => other.clone(),
    };

    match &ev {
        Event::Line { line } => {
            if log_enabled {
                if let Some(name) = &server_name {
                    st.logger.log(name, line);
                }
            }
            if notifications
                && !hidden
                && line.highlight
                && !line.own
                && !watched
                && matches!(line.kind, LineKind::Msg | LineKind::Action | LineKind::Notice | LineKind::Kick | LineKind::Info)
            {
                notify(app, line.buffer.as_str(), &line.nick, &line.text);
            }
        }
        Event::Joined { buffer } | Event::Query { buffer } if matches!(ev, Event::Joined { .. }) || open_queries => {
            // A server (or a flood of senders) must not be able to open windows without end:
            // beyond the cap the conversation still exists and is listed in the main menu.
            let wanted = Kind::Chat(server.to_string(), buffer.clone());
            let open_now = app.webview_windows().keys().filter(|l| l.starts_with("chat-")).count();
            if open_now < MAX_CHAT_WINDOWS || app.get_webview_window(&wanted.label()).is_some() {
                if let Err(e) = open(app, wanted, st.recent_input()) {
                    log::error!("fenêtre de {buffer} : {e}");
                }
            }
        }
        Event::Parted { buffer } => {
            if let Some(w) = app.get_webview_window(&Kind::Chat(server.to_string(), buffer.clone()).label()) {
                let _ = w.destroy();
            }
        }
        // The answers to /whois and /list come back in their own windows.
        Event::Whois { info } => {
            let _ = open(app, Kind::Whois(server.to_string(), info.nick.clone()), true);
        }
        Event::ListStart => {
            let _ = open(app, Kind::List(server.to_string()), st.recent_input());
        }
        // Someone calls: the window of their conversation opens, and the desktop tells.
        Event::Call { call } if call.state == crate::chat::event::CallState::Incoming => {
            let wanted = Kind::Chat(server.to_string(), call.buffer.clone());
            if let Err(e) = open(app, wanted, true) {
                log::error!("fenêtre de l'appel : {e}");
            }
            if notifications && !hidden {
                notify(app, call.buffer.as_str(), "", &format!("Appel {} de {}", if call.video { "vidéo" } else { "audio" }, call.text));
            }
        }
        // The code to link an account is shown in the window of the server's messages.
        Event::Pairing { .. } => {
            let _ = open(app, Kind::Console(server.to_string()), true);
        }
        _ => {}
    }

    // Seulement aux fenêtres qui peuvent en avoir besoin : celles de ce serveur (discussions, console, WHOIS, liste des salons) et les fenêtres
    // générales (lanceur, carnet, réglages, aller à…). Avant : chaque ligne de chaque serveur partait dans toutes les fenêtres ouvertes.
    let _ = app.emit_filter("chat", Payload { server, seq, event: &shown }, |target| match target {
        tauri::EventTarget::WebviewWindow { label } | tauri::EventTarget::Window { label } | tauri::EventTarget::Webview { label } => wants_server_events(label, server),
        _ => true,
    });
}

/// Does the window `label` need the events of `server` ? Its own windows (chat-<hex>-…, whois-<hex>-…, console-<hex>, list-<hex>), and
/// the general ones, whose label has no dash (launcher, servers, settings, about, join, switcher).
pub(super) fn wants_server_events(label: &str, server: &str) -> bool {
    if !label.contains('-') {
        return true;
    }
    let hex = text::label_part(server);
    label.starts_with(&format!("chat-{hex}-")) || label.starts_with(&format!("whois-{hex}-")) || label == format!("console-{hex}") || label == format!("list-{hex}")
}

fn notify(app: &AppHandle, buffer: &str, nick: &str, body: &str) {
    let st = app.state::<App>();
    {
        let mut last = st.last_notification.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < Duration::from_millis(800)) {
            return;
        }
        *last = Some(Instant::now());
    }
    let title = if buffer.is_empty() { "Messages".to_string() } else { text::truncate(buffer, 60) };
    let body = text::strip_formatting(body);
    let body = if nick.is_empty() { body } else { format!("{nick} : {body}") };
    crate::suite::notify_shell(&title, &body, "info"); // et dans le centre rapide du shell CDX, s'il tourne
    let _ = app.notification().builder().title(title).body(text::truncate(&body, 200)).show();
}
