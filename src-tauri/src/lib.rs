pub mod app;
pub mod commands;
pub mod config;
pub mod irc;
pub mod secrets;
pub mod store;
pub mod text;

use app::{App, Kind};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, RunEvent};

pub fn run() {
    let dir = config::data_dir();
    let (cfg, writable) = config::load_checked(&dir);
    let state = App::new(dir, cfg);
    state.set_writable(writable);

    let built = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            app::show_all(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_overview,
            commands::save_server,
            commands::delete_server,
            commands::connect_server,
            commands::disconnect_server,
            commands::send_input,
            commands::buffer_snapshot,
            commands::clear_buffer,
            commands::save_settings,
            commands::open_window,
            commands::open_url,
            commands::win_ready,
            commands::win_close,
            commands::win_shade,
            commands::win_drag,
            commands::hide_all,
            commands::quit_app,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            let show = MenuItem::with_id(app, "show", "Afficher KIRC", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("KIRC")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, ev| match ev.id().as_ref() {
                    "show" => app::show_all(app),
                    "quit" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(app::quit_all(app));
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        app::show_all(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            app::open(&handle, Kind::Launcher, true).map_err(|e| e.to_string())?;
            app::restore(&handle);

            // The configuration is written a moment after it changes, not on every pixel of a drag.
            let h2 = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    h2.state::<App>().flush();
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("KIRC n'a pas pu démarrer");

    built.run(|app, event| match event {
        // The launcher is hidden, never closed: the app stays alive in the tray.
        RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
        RunEvent::Exit => app.state::<App>().flush(),
        _ => {}
    });
}
