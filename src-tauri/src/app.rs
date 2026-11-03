//! The application: shared state, server connections, and the borderless
//! windows (one per function, like CDX) with their remembered geometry.

use crate::config::{self, Config, Geometry};
use crate::chat::event::{ConnState, Event, Line, LineKind};
use crate::chat::{self, fold, Command, Sink};
use crate::logs::Logger;
use crate::store::ServerState;
use crate::text;
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
    pub logger: Logger,
    /// (server id, buffer) of the windows looked at, newest first: the "Aller à" window lists them first.
    pub recent: Mutex<Vec<(String, String)>>,
    pub updates: crate::update::Updates,
    dirty: AtomicBool,
    writable: AtomicBool,
    save_lock: Mutex<()>,
    /// Per window, while its title bar is held: cursor and window position at the start.
    drags: Anchors<(i32, i32)>,
    /// Per window, while its grip is held: cursor and window size at the start.
    resizes: Anchors<(u32, u32)>,
    /// Rolled-up windows: the height to come back to, and whether the user could resize them.
    shaded: Mutex<HashMap<String, (u32, bool)>>,
    /// Windows merged into tabs (see tabs.rs).
    pub tabs: Mutex<crate::tabs::Tabs>,
    mins: Mutex<HashMap<String, (f64, f64)>>,
    last_input: Mutex<Option<Instant>>,
    last_notification: Mutex<Option<Instant>>,
    hidden: AtomicBool,
    quitting: AtomicBool,
}

impl App {
    pub fn new(dir: PathBuf, config: Config) -> Self {
        App {
            logger: Logger::new(dir.join("logs")),
            recent: Mutex::new(Vec::new()),
            updates: Default::default(),
            dir,
            config: Mutex::new(config),
            states: Mutex::new(HashMap::new()),
            servers: Mutex::new(HashMap::new()),
            dirty: AtomicBool::new(false),
            writable: AtomicBool::new(true),
            save_lock: Mutex::new(()),
            drags: Mutex::new(HashMap::new()),
            resizes: Mutex::new(HashMap::new()),
            shaded: Mutex::new(HashMap::new()),
            tabs: Mutex::new(Default::default()),
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

    /// Writes the configuration if it changed since the last time. Called from several threads (the
    /// 2 s loop, commands, exit): the lock covers the snapshot and the write, so an older snapshot
    /// can never be renamed over a newer one.
    pub fn flush(&self) {
        if !self.writable.load(Ordering::Relaxed) {
            return;
        }
        let _g = self.save_lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.dirty.swap(false, Ordering::AcqRel) {
            let cfg = self.config.lock().unwrap().clone();
            if let Err(e) = config::save(&self.dir, &cfg) {
                log::error!("config.json non enregistré : {e}");
                self.dirty.store(true, Ordering::Relaxed);
            }
        }
    }

    pub fn note_recent(&self, server: &str, buffer: &str) {
        let mut r = self.recent.lock().unwrap();
        r.retain(|(s, b)| !(s == server && fold(b) == fold(buffer)));
        r.insert(0, (server.to_string(), buffer.to_string()));
        r.truncate(60);
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.dir.join("logs")
    }

    /// The application did not exit after all (an installer that failed to start).
    pub fn reopen_after_failed_exit(&self) {
        self.quitting.store(false, Ordering::Relaxed);
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

mod events;
mod snap;
mod windows;
pub use events::{local_line, AppSink};
use events::handle_event;
#[cfg(test)]
use events::wants_server_events;
pub use snap::{drag, snap_rect, snap_size};
pub use windows::{close_server_windows, hide_all, open, show_all, toggle_shade, toggle_zoom, Kind};

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
    let open_console = settings.open_console;
    let sink = std::sync::Arc::new(AppSink { app: app.clone(), server: id.to_string() });
    // The protocol reads its credentials (the credential manager can take a moment) and returns what runs the account.
    let task = match chat::launch(chat::Start { dir: st.dir.clone(), id: id.to_string(), entry, settings, sink, commands: rx }) {
        Ok(task) => task,
        Err(e) => {
            st.servers.lock().unwrap().remove(id);
            return Err(e);
        }
    };
    let app2 = app.clone();
    let id2 = id.to_string();
    tauri::async_runtime::spawn(async move {
        task.await;
        app2.state::<App>().servers.lock().unwrap().remove(&id2);
    });

    if open_console {
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

/// Leaves every server politely (QUIT, up to 2.5 s) and saves; from then on the application is quitting.
pub async fn leave_servers(app: &AppHandle) {
    let st = app.state::<App>();
    st.quitting.store(true, Ordering::Relaxed);
    let ids: Vec<String> = st.servers.lock().unwrap().keys().cloned().collect();
    for id in &ids {
        stop_server(app, id);
    }
    for _ in 0..25 {
        if st.servers.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    st.flush();
}

/// Leaves every server politely, saves, and exits.
pub async fn quit_all(app: AppHandle) {
    leave_servers(&app).await;
    app.exit(0);
}

/// Everything to restore after the launcher is up: remembered windows and auto-connections.
pub fn restore(app: &AppHandle) {
    let st = app.state::<App>();
    let days = st.config.lock().unwrap().settings.log_days;
    let root = st.logs_dir();
    std::thread::spawn(move || {
        let n = crate::logs::sweep(&root, days);
        if n > 0 {
            log::info!("{n} journaux anciens supprimés");
        }
    });
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

#[cfg(test)]
mod tests {
    use super::{snap_rect, snap_size};
    use super::{wants_server_events, Kind};

    #[test]
    fn chat_events_only_reach_the_windows_of_their_server() {
        let a = Kind::Chat("srv-a".into(), "#salon".into()).label();
        let b = Kind::Chat("srv-b".into(), "#salon".into()).label();
        assert!(wants_server_events(&a, "srv-a"));
        assert!(!wants_server_events(&b, "srv-a"));
        assert!(wants_server_events(&Kind::Console("srv-a".into()).label(), "srv-a"));
        assert!(!wants_server_events(&Kind::Console("srv-b".into()).label(), "srv-a"));
        assert!(wants_server_events(&Kind::Whois("srv-a".into(), "bob".into()).label(), "srv-a"));
        assert!(wants_server_events(&Kind::List("srv-a".into()).label(), "srv-a"));
        for general in ["launcher", "servers", "settings", "about", "join", "switcher"] {
            assert!(wants_server_events(general, "srv-a"));
        }
    }

    // A 300 x 200 window "B" sits at (500, 100); windows are 200 x 100.
    const B: (i32, i32, i32, i32) = (500, 100, 300, 200);

    #[test]
    fn clings_to_the_side_of_another_window_edge_to_edge_with_the_real_gap() {
        assert_eq!(super::snap::SNAP_GAP, 0.0);
        // our right edge (x + 200) is 6 px short of B's left edge: it lands against it
        assert_eq!(snap_rect(294, 120, 200, 100, &[B], None, 10, 0), (300, 120));
        assert_eq!(snap_rect(805, 120, 200, 100, &[B], None, 10, 0), (800, 120));
        assert_eq!(snap_rect(520, 304, 200, 100, &[B], None, 10, 0), (520, 300));
        assert_eq!(snap_size(100, 120, 396, 100, &[B], None, 10, 0), (400, 100));
    }

    #[test]
    fn clings_to_the_side_of_another_window_leaving_the_gap() {
        // our right edge (x + 200) is 6 px short of B's left edge: it lands 2 px away from it
        assert_eq!(snap_rect(294, 120, 200, 100, &[B], None, 10, 2), (298, 120));
        // our left edge is 5 px inside B's right edge (800): it lands 2 px beyond it
        assert_eq!(snap_rect(805, 120, 200, 100, &[B], None, 10, 2), (802, 120));
    }

    #[test]
    fn clings_above_and_below_and_lines_edges_up() {
        assert_eq!(snap_rect(520, 304, 200, 100, &[B], None, 10, 2), (520, 302)); // under B
        assert_eq!(snap_rect(520, -4, 200, 100, &[B], None, 10, 2), (520, -2)); // above B: bottom edge 2 px above 100
        assert_eq!(snap_rect(503, 130, 200, 100, &[B], None, 10, 2), (500, 130)); // left edges in line
    }

    #[test]
    fn far_or_unrelated_windows_do_nothing() {
        assert_eq!(snap_rect(100, 120, 200, 100, &[B], None, 10, 2), (100, 120)); // too far
        // beside B's x range but far above it: nothing to cling to vertically, and not facing it horizontally
        assert_eq!(snap_rect(490, 600, 200, 100, &[B], None, 10, 2), (490, 600));
    }

    #[test]
    fn clings_to_the_screen_edges_without_a_gap_and_takes_the_closest() {
        let screen = Some((0, 0, 1920, 1040));
        assert_eq!(snap_rect(7, 6, 200, 100, &[], screen, 10, 2), (0, 0));
        assert_eq!(snap_rect(1715, 935, 200, 100, &[], screen, 10, 2), (1720, 940));
        // 4 px from the screen edge, but only 2 px from the other window's clinging spot: the closest wins
        assert_eq!(snap_rect(4, 120, 200, 100, &[(204, 100, 300, 200)], Some((0, 0, 1920, 1040)), 10, 2), (2, 120));
    }

    #[test]
    fn resizing_clings_the_right_and_bottom_edges() {
        // our window is at (100, 120); stretching it so that its right edge is 4 px short of B's left edge (500)
        assert_eq!(snap_size(100, 120, 396, 100, &[B], None, 10, 2), (398, 100)); // right edge ends 2 px from B
        // a bottom edge near the top of B's row would need B to be below: B is beside, so nothing happens
        assert_eq!(snap_size(100, 120, 200, 500, &[B], None, 10, 2), (200, 500));
        // right edges in line with B's right edge (800)
        assert_eq!(snap_size(100, 120, 697, 100, &[B], None, 10, 2), (700, 100));
        // against the screen
        assert_eq!(snap_size(100, 120, 1815, 915, &[], Some((0, 0, 1920, 1040)), 10, 2), (1820, 920));
        // far from everything
        assert_eq!(snap_size(100, 120, 300, 300, &[B], Some((0, 0, 1920, 1040)), 10, 2), (300, 300));
    }
}
