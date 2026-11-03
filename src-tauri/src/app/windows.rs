//! Les fenêtres sans cadre : leurs sortes, leur ouverture, leur géométrie retenue, enroulement et zoom. Sorti de app.rs (audit du 3/11/2026, 4.6).

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Launcher,
    Servers,
    Settings,
    About,
    Join,
    Console(String),
    /// The card of one person: (server id, nick).
    Whois(String, String),
    /// The channel list of a server.
    List(String),
    /// The "Aller à" window (Ctrl+K).
    Switcher,
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
            "whois" => Kind::Whois(need(server)?, need(buffer)?),
            "list" => Kind::List(need(server)?),
            "switcher" => Kind::Switcher,
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
            Kind::Whois(s, n) => format!("whois-{}-{}", text::label_part(s), text::label_part(&fold(n))),
            Kind::List(s) => format!("list-{}", text::label_part(s)),
            Kind::Switcher => "switcher".into(),
            Kind::Chat(s, b) => format!("chat-{}-{}", text::label_part(s), text::label_part(&fold(b))),
        }
    }

    /// Key of the remembered geometry (stable across runs).
    pub fn key(&self) -> String {
        match self {
            Kind::Console(s) => format!("console:{s}"),
            Kind::List(s) => format!("list:{s}"),
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
            Kind::Launcher => "Messages".into(),
            Kind::Servers => "Carnet d'adresses".into(),
            Kind::Settings => "Réglages".into(),
            Kind::About => "À propos de Messages".into(),
            Kind::Join => "Rejoindre un salon".into(),
            Kind::Console(s) => format!("Messages — {}", server_name(s)),
            Kind::Whois(s, n) => format!("{n} — {}", server_name(s)),
            Kind::List(s) => format!("Salons — {}", server_name(s)),
            Kind::Switcher => "Aller à…".into(),
            Kind::Chat(s, b) => format!("{b} — {}", server_name(s)),
        }
    }

    /// (width, height, min width, min height), logical pixels.
    fn size(&self) -> (f64, f64, f64, f64) {
        match self {
            Kind::Launcher => (190.0, 520.0, 190.0, 300.0),
            Kind::Servers => (640.0, 480.0, 520.0, 320.0),
            Kind::Settings => (440.0, 400.0, 400.0, 300.0),
            Kind::About => (340.0, 370.0, 340.0, 250.0),
            Kind::Join => (380.0, 190.0, 340.0, 150.0),
            Kind::Console(_) => (640.0, 360.0, 360.0, 160.0),
            Kind::Whois(_, _) => (400.0, 360.0, 400.0, 200.0),
            Kind::List(_) => (680.0, 480.0, 480.0, 240.0),
            Kind::Switcher => (500.0, 390.0, 500.0, 390.0),
            Kind::Chat(_, _) => (700.0, 420.0, 380.0, 180.0),
        }
    }

    fn resizable(&self) -> bool {
        !matches!(self, Kind::About | Kind::Join | Kind::Whois(_, _) | Kind::Switcher)
    }

    fn url(&self, title: &str, theme: &str) -> String {
        let t = format!("{}&th={}", text::url_encode(title), text::url_encode(theme));
        match self {
            Kind::Console(s) => format!("index.html?w=console&s={}&t={t}", text::url_encode(s)),
            Kind::Whois(s, n) => format!("index.html?w=whois&s={}&b={}&t={t}", text::url_encode(s), text::url_encode(n)),
            Kind::List(s) => format!("index.html?w=list&s={}&t={t}", text::url_encode(s)),
            Kind::Switcher => format!("index.html?w=switcher&t={t}"),
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
            if crate::tabs::is_hidden_tab(app, &label) {
                crate::tabs::activate(app, &label); // a tab behind another one: bring it forward
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
    let (remembered, theme) = {
        let c = st.config.lock().unwrap();
        (c.windows.get(&key).copied(), crate::suite::effective(&c.settings.theme))
    };
    let saved = remembered.filter(|g| on_screen(app, g)).filter(|_| kind != Kind::Switcher);
    let title = kind.title(app);
    let (w, h, min_w, min_h) = kind.size();
    let count = app.webview_windows().len() as f64;
    let step = (count % 8.0) * 26.0;

    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(kind.url(&title, &theme).into()))
        .title(&title)
        .inner_size(w, h)
        .min_inner_size(min_w, min_h)
        .decorations(false)
        .shadow(false)
        .resizable(kind.resizable())
        .skip_taskbar(!matches!(kind, Kind::Launcher))
        .always_on_top(kind == Kind::Switcher)
        .focused(focus && !hidden)
        .visible(false)
        .background_color(FRAME_BG)
        // Wry's default arguments, plus no LCD text: ClearType puts coloured fringes on the pixel font, whose
        // letters must be exactly white on the panel, as in CDX.
        .additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-lcd-text")
        .on_navigation(|url| url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost"));
    if matches!(kind, Kind::Launcher) {
        // the main window grows downwards only, to give the list of open channels more room
        b = b.max_inner_size(w, 4000.0);
    }
    if saved.is_none() {
        b = if matches!(kind, Kind::Launcher) { b.position(24.0, 24.0) } else { b.position(250.0 + step, 40.0 + step) };
    }
    let win = b.build().map_err(|e| e.to_string())?;
    if kind == Kind::Switcher {
        let _ = win.center();
    }
    st.mins.lock().unwrap().insert(label.clone(), (min_w, min_h));
    if let Some(g) = saved {
        // the main window only keeps its height: its width is fixed
        let gw = if matches!(kind, Kind::Launcher) { (w * win.scale_factor().unwrap_or(1.0)).round() as u32 } else { g.w };
        let _ = win.set_size(PhysicalSize::new(gw, g.h));
        let _ = win.set_position(PhysicalPosition::new(g.x, g.y));
    }

    let (app2, kind2, label2, key2) = (app.clone(), kind.clone(), label.clone(), key);
    win.on_window_event(move |ev| match ev {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) if kind2 != Kind::Switcher => remember_geometry(&app2, &label2, &key2),
        WindowEvent::Focused(f) => {
            let _ = app2.emit_to(label2.as_str(), "active", *f);
            // Looking at a conversation reads it.
            if *f {
                if let Kind::Chat(server, buffer) = &kind2 {
                    app2.state::<App>().note_recent(server, buffer);
                    handle_event(&app2, server, Event::Read { buffer: buffer.clone() });
                }
            }
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
    if win.is_minimized().unwrap_or(false) || win.is_maximized().unwrap_or(false) {
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
    crate::tabs::forget(app, label);
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
            let _ = send_command(app, server, Command::Input { buffer: buffer.clone(), text: "/part".into(), reply: None });
        }
    }
}

/// Closes the console and the chat windows of a server (deleted from the address book).
pub fn close_server_windows(app: &AppHandle, id: &str) {
    let hex = text::label_part(id);
    let (chat, console, whois, list) = (format!("chat-{hex}-"), format!("console-{hex}"), format!("whois-{hex}-"), format!("list-{hex}"));
    for (label, w) in app.webview_windows() {
        if label.starts_with(&chat) || label.starts_with(&whois) || label == console || label == list {
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
    for (label, w) in app.webview_windows() {
        if !crate::tabs::is_hidden_tab(app, &label) {
            let _ = w.show();
        }
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
        Some((full_h, resizable)) => {
            let _ = win.set_size(PhysicalSize::new(size.width, full_h));
            restore_min_size(app, label, &win);
            let _ = win.set_resizable(resizable);
            let _ = app.emit_to(label, "shade", false);
        }
        None => {
            st.shaded.lock().unwrap().insert(label.to_string(), (size.height, win.is_resizable().unwrap_or(true)));
            let _ = win.set_resizable(false);
            let _ = win.set_min_size(None::<tauri::LogicalSize<f64>>);
            let _ = win.set_size(PhysicalSize::new(size.width, (SHADED_HEIGHT * scale).round() as u32));
            let _ = app.emit_to(label, "shade", true);
        }
    }
}

/// The "+" of the title bar: fills the screen, or goes back to the previous size.
pub fn toggle_zoom(app: &AppHandle, label: &str) {
    if app.state::<App>().is_shaded(label) {
        return;
    }
    let Some(win) = app.get_webview_window(label) else { return };
    if win.is_maximized().unwrap_or(false) {
        let _ = win.unmaximize();
    } else {
        let _ = win.maximize();
    }
}

fn restore_min_size(app: &AppHandle, label: &str, win: &tauri::WebviewWindow) {
    let min = app.state::<App>().mins.lock().unwrap().get(label).copied();
    if let Some((w, h)) = min {
        let _ = win.set_min_size(Some(tauri::LogicalSize::new(w, h)));
    }
}
