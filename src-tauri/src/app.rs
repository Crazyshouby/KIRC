//! The application: shared state, server connections, and the borderless
//! windows (one per function, like KDX) with their remembered geometry.

use crate::config::{self, Config, Geometry};
use crate::irc::event::{ConnState, Event, LineKind};
use crate::irc::net::{self, Command, ConnectionConfig, Sink};
use crate::irc::session::{fold, Session, SessionConfig};
use crate::store::ServerState;
use crate::{secrets, text};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::mpsc::{self, UnboundedSender};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const FRAME_BG: Color = Color(0x22, 0x22, 0x22, 0xff);
/// Height of a rolled-up window: border, title bar and bottom frame, in logical pixels.
const SHADED_HEIGHT: f64 = 28.0;
/// Channel and conversation windows opened on their own; more stay reachable from the main menu.
const MAX_CHAT_WINDOWS: usize = 40;

type Point = (f64, f64);
type Anchors<T> = Mutex<HashMap<String, (Point, T)>>;

pub struct App {
    pub dir: PathBuf,
    pub config: Mutex<Config>,
    pub states: Mutex<HashMap<String, ServerState>>,
    pub servers: Mutex<HashMap<String, UnboundedSender<Command>>>,
    dirty: AtomicBool,
    writable: AtomicBool,
    /// Per window, while its title bar is held: cursor and window position at the start.
    drags: Anchors<(i32, i32)>,
    /// Per window, while its grip is held: cursor and window size at the start.
    resizes: Anchors<(u32, u32)>,
    shaded: Mutex<HashMap<String, u32>>,
    mins: Mutex<HashMap<String, (f64, f64)>>,
    last_input: Mutex<Option<Instant>>,
    last_notification: Mutex<Option<Instant>>,
    hidden: AtomicBool,
    quitting: AtomicBool,
}

impl App {
    pub fn new(dir: PathBuf, config: Config) -> Self {
        App {
            dir,
            config: Mutex::new(config),
            states: Mutex::new(HashMap::new()),
            servers: Mutex::new(HashMap::new()),
            dirty: AtomicBool::new(false),
            writable: AtomicBool::new(true),
            drags: Mutex::new(HashMap::new()),
            resizes: Mutex::new(HashMap::new()),
            shaded: Mutex::new(HashMap::new()),
            mins: Mutex::new(HashMap::new()),
            last_input: Mutex::new(None),
            last_notification: Mutex::new(None),
            hidden: AtomicBool::new(false),
            quitting: AtomicBool::new(false),
        }
    }

    /// Turned off when the existing configuration could not be read (see `config::load_checked`).
    pub fn set_writable(&self, writable: bool) {
        self.writable.store(writable, Ordering::Relaxed);
    }

    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Writes the configuration if it changed since the last time.
    pub fn flush(&self) {
        if !self.writable.load(Ordering::Relaxed) {
            return;
        }
        if self.dirty.swap(false, Ordering::Relaxed) {
            let cfg = self.config.lock().unwrap().clone();
            if let Err(e) = config::save(&self.dir, &cfg) {
                log::error!("config.json non enregistré : {e}");
                self.dirty.store(true, Ordering::Relaxed);
            }
        }
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::Relaxed)
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden.load(Ordering::Relaxed)
    }

    /// Something was typed a moment ago: a window that opens now was asked for.
    pub fn note_input(&self) {
        *self.last_input.lock().unwrap() = Some(Instant::now());
    }

    fn recent_input(&self) -> bool {
        self.last_input.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_secs(3))
    }

    pub fn is_shaded(&self, label: &str) -> bool {
        self.shaded.lock().unwrap().contains_key(label)
    }
}

// ------------------------------------------------------------------ events

#[derive(Serialize, Clone)]
struct Payload<'a> {
    server: &'a str,
    seq: u64,
    event: &'a Event,
}

pub struct IrcSink {
    pub app: AppHandle,
    pub server: String,
}

impl Sink for IrcSink {
    fn emit(&self, event: Event) {
        handle_event(&self.app, &self.server, event);
    }
}

fn handle_event(app: &AppHandle, server: &str, ev: Event) {
    let st = app.state::<App>();
    let (open_queries, notifications) = {
        let c = st.config.lock().unwrap();
        (c.settings.open_queries, c.settings.notifications)
    };
    let seq = {
        let mut states = st.states.lock().unwrap();
        // A server deleted while its connection winds down must not come back as a ghost.
        let Some(s) = states.get_mut(server) else { return };
        s.apply(&ev);
        s.seq
    };

    match &ev {
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
        Event::Line { line } if notifications && line.highlight && !line.own => {
            let watched = app
                .get_webview_window(&Kind::Chat(server.to_string(), line.buffer.clone()).label())
                .is_some_and(|w| w.is_focused().unwrap_or(false));
            if !watched && matches!(line.kind, LineKind::Msg | LineKind::Action | LineKind::Notice | LineKind::Kick | LineKind::Info) {
                notify(app, line.buffer.as_str(), &line.nick, &line.text);
            }
        }
        _ => {}
    }

    let _ = app.emit("irc", Payload { server, seq, event: &ev });
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
    let title = if buffer.is_empty() { "KIRC".to_string() } else { text::truncate(buffer, 60) };
    let body = text::strip_formatting(body);
    let body = if nick.is_empty() { body } else { format!("{nick} : {body}") };
    let _ = app.notification().builder().title(title).body(text::truncate(&body, 200)).show();
}

// ----------------------------------------------------------------- servers

/// Connects one server of the address book.
pub fn start_server(app: &AppHandle, id: &str) -> Result<(), String> {
    let st = app.state::<App>();
    let (entry, settings) = {
        let c = st.config.lock().unwrap();
        (c.server(id).cloned().ok_or("Serveur inconnu")?, c.settings.clone())
    };
    // Reserve the connection before the slow part (the credential manager): two quick clicks
    // must not start two sessions with the same nick.
    let (tx, rx) = mpsc::unbounded_channel();
    {
        let mut servers = st.servers.lock().unwrap();
        if servers.contains_key(id) {
            return Err("Ce serveur est déjà connecté".into());
        }
        servers.insert(id.to_string(), tx);
    }
    {
        let mut states = st.states.lock().unwrap();
        states.entry(id.to_string()).or_insert_with(|| ServerState::new(settings.scrollback as usize));
    }
    let pick = |own: &str, default: &str| if own.trim().is_empty() { default.to_string() } else { own.trim().to_string() };
    let nick = pick(&entry.nick, &settings.nick);
    let alt = pick(&entry.alt_nick, &settings.alt_nick);
    let auth_secret = secrets::get(id, "auth");
    let (sasl, nickserv_password) = match (entry.auth.as_str(), auth_secret) {
        ("sasl", Some(p)) => (Some((pick(&entry.account, &nick), p)), None),
        ("nickserv", Some(p)) => (None, Some(p)),
        _ => (None, None),
    };
    let cfg = SessionConfig {
        nick: nick.clone(),
        alt_nicks: if alt.is_empty() { Vec::new() } else { vec![alt] },
        user: pick(&entry.username, &settings.username),
        realname: pick(&entry.realname, &settings.realname),
        password: secrets::get(id, "pass"),
        sasl,
        nickserv_password,
        autojoin: entry.autojoin_list(),
        quit_message: settings.quit_message.clone(),
        part_message: settings.part_message.clone(),
        version: format!("KIRC {VERSION}"),
    };
    let conn = ConnectionConfig {
        host: entry.host.clone(),
        port: entry.port,
        tls: entry.tls,
        accept_invalid_certs: entry.accept_invalid_certs,
        autoreconnect: true,
    };

    let sink = std::sync::Arc::new(IrcSink { app: app.clone(), server: id.to_string() });
    let app2 = app.clone();
    let id2 = id.to_string();
    tauri::async_runtime::spawn(async move {
        net::run(conn, Session::new(cfg), sink, rx).await;
        app2.state::<App>().servers.lock().unwrap().remove(&id2);
    });

    if settings.open_console {
        let _ = open(app, Kind::Console(id.to_string()), false);
    }
    Ok(())
}

pub fn stop_server(app: &AppHandle, id: &str) {
    let st = app.state::<App>();
    let quit = st.config.lock().unwrap().settings.quit_message.clone();
    let servers = st.servers.lock().unwrap();
    if let Some(tx) = servers.get(id) {
        let _ = tx.send(Command::Disconnect(quit));
    }
}

pub fn send_command(app: &AppHandle, id: &str, cmd: Command) -> Result<(), String> {
    let st = app.state::<App>();
    let servers = st.servers.lock().unwrap();
    match servers.get(id) {
        Some(tx) => tx.send(cmd).map_err(|_| "Non connecté".to_string()),
        None => Err("Non connecté : connectez d'abord le serveur".into()),
    }
}

/// Leaves every server politely, saves, and exits.
pub async fn quit_all(app: AppHandle) {
    let st = app.state::<App>();
    st.quitting.store(true, Ordering::Relaxed);
    let ids: Vec<String> = st.servers.lock().unwrap().keys().cloned().collect();
    for id in &ids {
        stop_server(&app, id);
    }
    for _ in 0..25 {
        if st.servers.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    st.flush();
    app.exit(0);
}

// ----------------------------------------------------------------- windows

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Launcher,
    Servers,
    Settings,
    About,
    Join,
    Console(String),
    /// A channel or a private conversation: (server id, buffer).
    Chat(String, String),
}

impl Kind {
    pub fn parse(kind: &str, server: Option<String>, buffer: Option<String>) -> Result<Kind, String> {
        let need = |s: Option<String>| s.filter(|s| !s.is_empty()).ok_or_else(|| "Paramètre manquant".to_string());
        Ok(match kind {
            "launcher" => Kind::Launcher,
            "servers" => Kind::Servers,
            "settings" => Kind::Settings,
            "about" => Kind::About,
            "join" => Kind::Join,
            "console" => Kind::Console(need(server)?),
            "chat" => Kind::Chat(need(server)?, need(buffer)?),
            _ => return Err("Fenêtre inconnue".into()),
        })
    }

    pub fn label(&self) -> String {
        match self {
            Kind::Launcher => "launcher".into(),
            Kind::Servers => "servers".into(),
            Kind::Settings => "settings".into(),
            Kind::About => "about".into(),
            Kind::Join => "join".into(),
            Kind::Console(s) => format!("console-{}", text::label_part(s)),
            Kind::Chat(s, b) => format!("chat-{}-{}", text::label_part(s), text::label_part(&fold(b))),
        }
    }

    /// Key of the remembered geometry (stable across runs).
    pub fn key(&self) -> String {
        match self {
            Kind::Console(s) => format!("console:{s}"),
            Kind::Chat(s, b) => format!("chat:{s}:{}", fold(b)),
            other => other.label(),
        }
    }

    fn title(&self, app: &AppHandle) -> String {
        let server_name = |id: &str| {
            let st = app.state::<App>();
            let c = st.config.lock().unwrap();
            c.server(id).map_or_else(|| id.to_string(), |s| s.name.clone())
        };
        match self {
            Kind::Launcher => "KIRC".into(),
            Kind::Servers => "Carnet d'adresses".into(),
            Kind::Settings => "Réglages".into(),
            Kind::About => "À propos de KIRC".into(),
            Kind::Join => "Rejoindre un salon".into(),
            Kind::Console(s) => format!("Messages — {}", server_name(s)),
            Kind::Chat(s, b) => format!("{b} — {}", server_name(s)),
        }
    }

    /// (width, height, min width, min height), logical pixels.
    fn size(&self) -> (f64, f64, f64, f64) {
        match self {
            Kind::Launcher => (206.0, 438.0, 206.0, 80.0),
            Kind::Servers => (640.0, 480.0, 520.0, 320.0),
            Kind::Settings => (440.0, 580.0, 400.0, 300.0),
            Kind::About => (340.0, 300.0, 340.0, 200.0),
            Kind::Join => (380.0, 190.0, 340.0, 150.0),
            Kind::Console(_) => (640.0, 360.0, 360.0, 160.0),
            Kind::Chat(_, _) => (700.0, 420.0, 380.0, 180.0),
        }
    }

    fn resizable(&self) -> bool {
        !matches!(self, Kind::Launcher | Kind::About | Kind::Join)
    }

    fn url(&self, title: &str) -> String {
        let t = text::url_encode(title);
        match self {
            Kind::Console(s) => format!("index.html?w=console&s={}&t={t}", text::url_encode(s)),
            Kind::Chat(s, b) => format!("index.html?w=chat&s={}&b={}&t={t}", text::url_encode(s), text::url_encode(b)),
            Kind::Launcher => format!("index.html?w=launcher&t={t}"),
            Kind::Servers => format!("index.html?w=servers&t={t}"),
            Kind::Settings => format!("index.html?w=settings&t={t}"),
            Kind::About => format!("index.html?w=about&t={t}"),
            Kind::Join => format!("index.html?w=join&t={t}"),
        }
    }

    /// Windows that come back at the next start.
    fn persistent(&self) -> Option<&'static str> {
        match self {
            Kind::Servers => Some("servers"),
            Kind::Settings => Some("settings"),
            _ => None,
        }
    }
}

fn on_screen(app: &AppHandle, g: &Geometry) -> bool {
    let Ok(monitors) = app.available_monitors() else { return true };
    if monitors.is_empty() {
        return true;
    }
    monitors.iter().any(|m| {
        let (mx, my, mw, mh) = (m.position().x as i64, m.position().y as i64, m.size().width as i64, m.size().height as i64);
        let (x, y, w, h) = (g.x as i64, g.y as i64, g.w as i64, g.h as i64);
        let ox = (x + w).min(mx + mw) - x.max(mx);
        let oy = (y + h).min(my + mh) - y.max(my);
        ox >= 120 && oy >= 40
    })
}

/// Opens a window, or brings the existing one to the front.
pub fn open(app: &AppHandle, kind: Kind, focus: bool) -> Result<(), String> {
    let st = app.state::<App>();
    let label = kind.label();
    if let Some(w) = app.get_webview_window(&label) {
        if focus {
            if st.is_hidden() {
                show_all(app);
            }
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        return Ok(());
    }
    let hidden = st.is_hidden();
    let key = kind.key();
    // The lock is released before asking the monitors: that call waits for the main thread,
    // which may itself be waiting for this lock (a window moved at that moment).
    let remembered = st.config.lock().unwrap().windows.get(&key).copied();
    let saved = remembered.filter(|g| on_screen(app, g));
    let title = kind.title(app);
    let (w, h, min_w, min_h) = kind.size();
    let count = app.webview_windows().len() as f64;
    let step = (count % 8.0) * 26.0;

    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(kind.url(&title).into()))
        .title(&title)
        .inner_size(w, h)
        .min_inner_size(min_w, min_h)
        .decorations(false)
        .shadow(false)
        .resizable(kind.resizable())
        .skip_taskbar(!matches!(kind, Kind::Launcher))
        .focused(focus && !hidden)
        .visible(false)
        .background_color(FRAME_BG)
        .on_navigation(|url| url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost"));
    if saved.is_none() {
        b = if matches!(kind, Kind::Launcher) { b.position(24.0, 24.0) } else { b.position(250.0 + step, 40.0 + step) };
    }
    let win = b.build().map_err(|e| e.to_string())?;
    st.mins.lock().unwrap().insert(label.clone(), (min_w, min_h));
    if let Some(g) = saved {
        let _ = win.set_size(PhysicalSize::new(g.w, g.h));
        let _ = win.set_position(PhysicalPosition::new(g.x, g.y));
    }

    let (app2, kind2, label2, key2) = (app.clone(), kind.clone(), label.clone(), key);
    win.on_window_event(move |ev| match ev {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => remember_geometry(&app2, &label2, &key2),
        WindowEvent::Focused(f) => {
            let _ = app2.emit_to(label2.as_str(), "active", *f);
        }
        WindowEvent::CloseRequested { api, .. } => {
            if matches!(kind2, Kind::Launcher) && !app2.state::<App>().is_quitting() {
                api.prevent_close();
                hide_all(&app2);
            }
        }
        WindowEvent::Destroyed => on_destroyed(&app2, &kind2, &label2),
        _ => {}
    });

    if !hidden {
        let _ = win.show();
        if focus {
            let _ = win.set_focus();
        }
    }
    if let Some(name) = kind.persistent() {
        let mut c = st.config.lock().unwrap();
        if !c.open_windows.iter().any(|n| n == name) {
            c.open_windows.push(name.to_string());
        }
        drop(c);
        st.mark_dirty();
    }
    Ok(())
}

fn remember_geometry(app: &AppHandle, label: &str, key: &str) {
    let st = app.state::<App>();
    if st.is_shaded(label) || st.is_quitting() {
        return;
    }
    let Some(win) = app.get_webview_window(label) else { return };
    if win.is_minimized().unwrap_or(false) {
        return;
    }
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else { return };
    if size.width < 120 || size.height < 60 || pos.x < -30000 {
        return;
    }
    st.config.lock().unwrap().remember_window(key, Geometry { x: pos.x, y: pos.y, w: size.width, h: size.height });
    st.mark_dirty();
}

fn on_destroyed(app: &AppHandle, kind: &Kind, label: &str) {
    let st = app.state::<App>();
    st.shaded.lock().unwrap().remove(label);
    st.mins.lock().unwrap().remove(label);
    st.drags.lock().unwrap().remove(label);
    st.resizes.lock().unwrap().remove(label);
    if st.is_quitting() {
        return;
    }
    if let Some(name) = kind.persistent() {
        st.config.lock().unwrap().open_windows.retain(|n| n != name);
        st.mark_dirty();
    }
    // Closing the window of a channel leaves the channel.
    if let Kind::Chat(server, buffer) = kind {
        let joined = st.states.lock().unwrap().get(server).is_some_and(|s| s.summary().channels.iter().any(|c| fold(c) == fold(buffer)));
        if joined {
            let _ = send_command(app, server, Command::Input { buffer: buffer.clone(), text: "/part".into() });
        }
    }
}

/// Closes the console and the chat windows of a server (deleted from the address book).
pub fn close_server_windows(app: &AppHandle, id: &str) {
    let hex = text::label_part(id);
    let (chat, console) = (format!("chat-{hex}-"), format!("console-{hex}"));
    for (label, w) in app.webview_windows() {
        if label.starts_with(&chat) || label == console {
            let _ = w.destroy();
        }
    }
}

pub fn hide_all(app: &AppHandle) {
    let st = app.state::<App>();
    st.hidden.store(true, Ordering::Relaxed);
    for w in app.webview_windows().values() {
        let _ = w.hide();
    }
}

pub fn show_all(app: &AppHandle) {
    let st = app.state::<App>();
    st.hidden.store(false, Ordering::Relaxed);
    for w in app.webview_windows().values() {
        let _ = w.show();
    }
    if let Some(l) = app.get_webview_window("launcher") {
        let _ = l.unminimize();
        let _ = l.set_focus();
    }
}

/// Rolls a window up to its title bar, or back down.
pub fn toggle_shade(app: &AppHandle, label: &str) {
    let st = app.state::<App>();
    let Some(win) = app.get_webview_window(label) else { return };
    let scale = win.scale_factor().unwrap_or(1.0);
    let Ok(size) = win.inner_size() else { return };
    let was = st.shaded.lock().unwrap().remove(label);
    match was {
        Some(full_h) => {
            let _ = win.set_size(PhysicalSize::new(size.width, full_h));
            restore_min_size(app, label, &win);
            let _ = win.set_resizable(true);
            let _ = app.emit_to(label, "shade", false);
        }
        None => {
            st.shaded.lock().unwrap().insert(label.to_string(), size.height);
            let _ = win.set_resizable(false);
            let _ = win.set_min_size(None::<tauri::LogicalSize<f64>>);
            let _ = win.set_size(PhysicalSize::new(size.width, (SHADED_HEIGHT * scale).round() as u32));
            let _ = app.emit_to(label, "shade", true);
        }
    }
}

fn restore_min_size(app: &AppHandle, label: &str, win: &tauri::WebviewWindow) {
    let min = app.state::<App>().mins.lock().unwrap().get(label).copied();
    if let Some((w, h)) = min {
        let _ = win.set_min_size(Some(tauri::LogicalSize::new(w, h)));
    }
}

pub fn drag(app: &AppHandle, label: &str, phase: &str) {
    let st = app.state::<App>();
    let Some(win) = app.get_webview_window(label) else { return };
    match phase {
        "start" => {
            if let (Ok(c), Ok(p)) = (win.cursor_position(), win.outer_position()) {
                st.drags.lock().unwrap().insert(label.to_string(), ((c.x, c.y), (p.x, p.y)));
            }
        }
        "move" => {
            let start = st.drags.lock().unwrap().get(label).copied();
            if let (Some(((cx, cy), (wx, wy))), Ok(c)) = (start, win.cursor_position()) {
                let _ = win.set_position(PhysicalPosition::new(wx + (c.x - cx).round() as i32, wy + (c.y - cy).round() as i32));
            }
        }
        "rstart" => {
            if let (Ok(c), Ok(sz)) = (win.cursor_position(), win.inner_size()) {
                st.resizes.lock().unwrap().insert(label.to_string(), ((c.x, c.y), (sz.width, sz.height)));
            }
        }
        "rmove" => {
            let start = st.resizes.lock().unwrap().get(label).copied();
            let min = st.mins.lock().unwrap().get(label).copied().unwrap_or((200.0, 100.0));
            let scale = win.scale_factor().unwrap_or(1.0);
            if let (Some(((cx, cy), (w, h))), Ok(c)) = (start, win.cursor_position()) {
                let nw = (w as f64 + (c.x - cx)).max(min.0 * scale).round() as u32;
                let nh = (h as f64 + (c.y - cy)).max(min.1 * scale).round() as u32;
                let _ = win.set_size(PhysicalSize::new(nw, nh));
            }
        }
        _ => {
            st.drags.lock().unwrap().remove(label);
            st.resizes.lock().unwrap().remove(label);
        }
    }
}

/// Everything to restore after the launcher is up: remembered windows and auto-connections.
pub fn restore(app: &AppHandle) {
    let st = app.state::<App>();
    let (names, autoconnect): (Vec<String>, Vec<String>) = {
        let c = st.config.lock().unwrap();
        (c.open_windows.clone(), c.servers.iter().filter(|s| s.autoconnect).map(|s| s.id.clone()).collect())
    };
    // A first start has nothing to connect to: show where servers are added.
    let empty = st.config.lock().unwrap().servers.is_empty();
    if empty && !names.iter().any(|n| n == "servers") {
        let _ = open(app, Kind::Servers, false);
    }
    for n in names {
        if let Ok(k) = Kind::parse(&n, None, None) {
            let _ = open(app, k, false);
        }
    }
    for id in autoconnect {
        if let Err(e) = start_server(app, &id) {
            log::warn!("connexion automatique de {id} : {e}");
        }
    }
}

pub fn status_of(app: &AppHandle, id: &str) -> ConnState {
    app.state::<App>().states.lock().unwrap().get(id).map_or(ConnState::Disconnected, |s| s.status)
}
