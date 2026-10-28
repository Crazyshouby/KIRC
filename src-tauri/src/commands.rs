//! The functions the pages can call.

use crate::app::{self, App, Kind, VERSION};
use crate::config::{ServerEntry, Settings};
use crate::irc::net::Command;
use crate::irc::session::fold;
use crate::irc::event::{LineKind, WhoisInfo};
use crate::store::{BufferState, ListPage, ServerSummary};
use crate::{secrets, text};
use serde::Serialize;
use std::collections::HashMap;
use tauri::{AppHandle, Manager, State, WebviewWindow};

type R<T> = Result<T, String>;

#[derive(Serialize)]
pub struct ServerView {
    #[serde(flatten)]
    entry: ServerEntry,
    has_pass: bool,
    has_auth: bool,
}

#[derive(Serialize)]
pub struct Overview {
    settings: Settings,
    servers: Vec<ServerView>,
    states: HashMap<String, ServerSummary>,
}

#[derive(Serialize)]
pub struct Snapshot {
    seq: u64,
    server: ServerSummary,
    buffer: BufferState,
    settings: Settings,
}

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    data_dir: String,
}

#[tauri::command]
pub fn app_info(state: State<'_, App>) -> AppInfo {
    AppInfo { version: VERSION, data_dir: state.dir.display().to_string() }
}

#[tauri::command]
pub async fn get_overview(state: State<'_, App>) -> R<Overview> {
    let (settings, entries) = {
        let c = state.config.lock().unwrap();
        (c.settings.clone(), c.servers.clone())
    };
    // The credential manager is asked outside the lock: it can take a moment.
    let servers = entries
        .into_iter()
        .map(|entry| {
            let has_pass = secrets::get(&entry.id, "pass").is_some();
            let has_auth = secrets::get(&entry.id, "auth").is_some();
            ServerView { entry, has_pass, has_auth }
        })
        .collect();
    let states = state.states.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.summary())).collect();
    Ok(Overview { settings, servers, states })
}

#[derive(Serialize)]
pub struct SaveResult {
    id: String,
    /// The entry is saved, but something around it failed (the credential manager).
    warning: Option<String>,
}

/// `pass` and `auth_secret`: None keeps the stored password, "" removes it.
#[tauri::command]
pub async fn save_server(state: State<'_, App>, entry: ServerEntry, pass: Option<String>, auth_secret: Option<String>) -> R<SaveResult> {
    let id = state.config.lock().unwrap().upsert_server(entry)?;
    state.mark_dirty();
    state.flush();
    let mut problems = Vec::new();
    for (kind, value) in [("pass", pass), ("auth", auth_secret)] {
        if let Some(v) = value {
            if let Err(e) = secrets::set(&id, kind, &v) {
                problems.push(e);
            }
        }
    }
    let warning = (!problems.is_empty()).then(|| format!("Serveur enregistré, mais {}", problems.join(" ; ")));
    Ok(SaveResult { id, warning })
}

#[tauri::command]
pub async fn delete_server(app: AppHandle, state: State<'_, App>, id: String) -> R<bool> {
    app::stop_server(&app, &id);
    state.config.lock().unwrap().remove_server(&id);
    state.states.lock().unwrap().remove(&id);
    state.mark_dirty();
    state.flush();
    secrets::forget_server(&id);
    app::close_server_windows(&app, &id);
    Ok(true)
}

#[tauri::command]
pub async fn connect_server(app: AppHandle, id: String) -> R<()> {
    app::start_server(&app, &id)
}

#[tauri::command]
pub fn disconnect_server(app: AppHandle, id: String) {
    app::stop_server(&app, &id);
}

#[tauri::command]
pub fn send_input(app: AppHandle, state: State<'_, App>, server: String, buffer: String, text: String) -> R<bool> {
    if text.len() > 4000 {
        return Err("Ligne trop longue (4000 octets au plus)".into());
    }
    state.note_input();
    if let Some(done) = ignore_command(&app, &state, &server, &buffer, &text) {
        return done.map(|()| true);
    }
    app::send_command(&app, &server, Command::Input { buffer, text })?;
    Ok(true)
}

/// `/ignore [mask]` and `/unignore mask`: the lists live in the settings and apply to every server.
/// None when the line is not one of these commands.
fn ignore_command(app: &AppHandle, state: &App, server: &str, buffer: &str, text: &str) -> Option<Result<(), String>> {
    let t = text.trim();
    let (cmd, arg) = t.split_once(' ').map_or((t, ""), |(c, a)| (c, a.trim()));
    let cmd = cmd.to_ascii_lowercase();
    if cmd != "/ignore" && cmd != "/unignore" {
        return None;
    }
    let say = |kind: LineKind, msg: &str| app::local_line(app, server, buffer, kind, msg);
    let mut cfg = state.config.lock().unwrap();
    if arg.is_empty() {
        let list = cfg.settings.ignores.clone();
        drop(cfg);
        say(LineKind::Info, &if list.is_empty() { "Aucun pseudo ignoré".to_string() } else { format!("Ignorés : {}", list.join(", ")) });
        return Some(Ok(()));
    }
    let key = crate::irc::session::fold(arg);
    let mut next = cfg.settings.clone();
    if cmd == "/ignore" {
        if !next.ignores.iter().any(|i| crate::irc::session::fold(i) == key) {
            next.ignores.push(arg.to_string());
        }
    } else {
        next.ignores.retain(|i| crate::irc::session::fold(i) != key);
    }
    if let Err(e) = next.validate() {
        drop(cfg);
        say(LineKind::Error, &e);
        return Some(Ok(()));
    }
    cfg.settings = next;
    let (ignores, words) = (cfg.settings.ignores.clone(), cfg.settings.alert_words.clone());
    drop(cfg);
    state.mark_dirty();
    push_filters(app, state, &ignores, &words);
    say(LineKind::Info, &if cmd == "/ignore" { format!("{arg} est ignoré") } else { format!("{arg} n'est plus ignoré") });
    Some(Ok(()))
}

/// Tells every running connection about new ignore and alert lists.
fn push_filters(app: &AppHandle, state: &App, ignores: &[String], words: &[String]) {
    let _ = app;
    for tx in state.servers.lock().unwrap().values() {
        let _ = tx.send(Command::SetFilters { ignores: ignores.to_vec(), alert_words: words.to_vec() });
    }
}

#[tauri::command]
pub fn buffer_snapshot(state: State<'_, App>, server: String, buffer: String) -> R<Snapshot> {
    let settings = state.config.lock().unwrap().settings.clone();
    let states = state.states.lock().unwrap();
    let s = states.get(&server).ok_or("Serveur inconnu")?;
    Ok(Snapshot { seq: s.seq, server: s.summary(), buffer: s.snapshot(&buffer), settings })
}

#[tauri::command]
pub fn clear_buffer(state: State<'_, App>, server: String, buffer: String) {
    if let Some(s) = state.states.lock().unwrap().get_mut(&server) {
        s.clear_lines(&buffer);
    }
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, state: State<'_, App>, settings: Settings) -> R<bool> {
    settings.validate()?;
    push_filters(&app, &state, &settings.ignores, &settings.alert_words);
    let theme_changed = state.config.lock().unwrap().settings.theme != settings.theme;
    let theme = settings.theme.clone();
    let n = settings.scrollback as usize;
    state.config.lock().unwrap().settings = settings;
    for s in state.states.lock().unwrap().values_mut() {
        s.set_scrollback(n);
    }
    state.mark_dirty();
    state.flush();
    if theme_changed {
        let _ = tauri::Emitter::emit(&app, "theme", theme);
    }
    Ok(true)
}

#[tauri::command]
pub async fn open_window(app: AppHandle, kind: String, server: Option<String>, buffer: Option<String>) -> R<()> {
    let kind = Kind::parse(&kind, server, buffer)?;
    if let Kind::Chat(s, b) = &kind {
        // A private conversation can be started from anywhere; a channel needs /join.
        if !app.state::<App>().states.lock().unwrap().contains_key(s) {
            return Err("Serveur inconnu".into());
        }
        let _ = (b, fold(b));
    }
    app::open(&app, kind, true)
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> R<()> {
    if !text::is_web_url(&url) {
        return Err("Seuls les liens http et https peuvent être ouverts".into());
    }
    tauri_plugin_opener::OpenerExt::opener(&app).open_url(url, None::<&str>).map_err(|e| e.to_string())
}

// ------------------------------------------------------------ window chrome

#[tauri::command]
pub fn win_ready(app: AppHandle, window: WebviewWindow) {
    let label = window.label().to_string();
    let _ = tauri::Emitter::emit_to(&app, label.as_str(), "active", window.is_focused().unwrap_or(false));
}

#[tauri::command]
pub fn win_close(app: AppHandle, window: WebviewWindow) {
    if window.label() == "launcher" {
        app::hide_all(&app);
    } else {
        let _ = window.close();
    }
}

#[tauri::command]
pub fn win_shade(app: AppHandle, window: WebviewWindow) {
    app::toggle_shade(&app, window.label());
}

#[tauri::command]
pub fn win_drag(app: AppHandle, window: WebviewWindow, phase: String) {
    app::drag(&app, window.label(), &phase);
}

#[tauri::command]
pub fn hide_all(app: AppHandle) {
    app::hide_all(&app);
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) {
    app::quit_all(app).await;
}

/// Just the connection states (no credential manager involved): cheap enough to ask often.
#[tauri::command]
pub fn get_states(state: State<'_, App>) -> HashMap<String, ServerSummary> {
    state.states.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.summary())).collect()
}

#[tauri::command]
pub fn get_channel_list(state: State<'_, App>, server: String, filter: String, min_users: u32, by_name: bool, offset: usize, limit: usize) -> R<ListPage> {
    let states = state.states.lock().unwrap();
    let s = states.get(&server).ok_or("Serveur inconnu")?;
    Ok(s.list_page(&filter, min_users, by_name, offset, limit))
}

#[tauri::command]
pub fn get_whois(state: State<'_, App>, server: String, nick: String) -> R<WhoisInfo> {
    let states = state.states.lock().unwrap();
    states.get(&server).and_then(|s| s.whois(&nick)).ok_or_else(|| "Aucune information".to_string())
}

/// The end of the log of a conversation, older than `before` (ms), to show above the live lines.
#[tauri::command]
pub async fn read_backlog(state: State<'_, App>, server: String, buffer: String, before: i64) -> R<Vec<crate::irc::event::Line>> {
    let (enabled, name) = {
        let c = state.config.lock().unwrap();
        (c.settings.log_enabled, c.server(&server).map(|e| e.name.clone()))
    };
    let Some(name) = name.filter(|_| enabled && !buffer.is_empty()) else { return Ok(Vec::new()) };
    state.logger.flush();
    Ok(crate::logs::tail(&state.logs_dir(), &name, &buffer, before, 100))
}

#[tauri::command]
pub fn open_logs_folder(app: AppHandle, state: State<'_, App>) -> R<()> {
    let dir = state.logs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    tauri_plugin_opener::OpenerExt::opener(&app).open_path(dir.display().to_string(), None::<&str>).map_err(|e| e.to_string())
}
