//! L'accent de la suite CDX (`%APPDATA%\CDX\suite.json`, voir cdx-ui/docs/SUITE.md).
//!
//! KIRC suit l'accent de la suite tant que son réglage « thème » est sur sa valeur par défaut (`cdx`) ; un thème choisi à la main reste prioritaire.
//! Chez KIRC le rouge de la suite s'appelle `cdx` (le look par défaut) ; les autres accents portent le même nom des deux côtés.

use crate::app::App;
use crate::cdx_bus;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

static ACCENT: Mutex<String> = Mutex::new(String::new());

/// Lit l'accent de la suite (à appeler avant d'ouvrir une fenêtre).
pub fn init() {
    *ACCENT.lock().unwrap_or_else(|e| e.into_inner()) = cdx_bus::read_accent("rouge");
}

fn current() -> String {
    let a = ACCENT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if a.is_empty() { "rouge".into() } else { a }
}

/// Le nom KIRC d'un accent de la suite.
pub fn to_kirc(accent: &str) -> String {
    if accent == "rouge" { "cdx".into() } else { accent.to_string() }
}

/// Le thème que les fenêtres doivent porter : celui du réglage, ou l'accent de la suite si le réglage est sur `cdx`.
pub fn effective(setting: &str) -> String {
    if setting == "cdx" { to_kirc(&current()) } else { setting.to_string() }
}

/// Ouvre un lien dans le navigateur CDX : par le bus s'il tourne, sinon en le lançant s'il est installé (apps.json). `false` : l'appelant se rabat sur
/// l'ouvreur du système. Seuls les liens http et https arrivent ici (voir `commands::open_url`).
pub fn open_in_cdx_browser(url: &str) -> bool {
    if cdx_bus::send("browser", "open-url", &serde_json::json!({ "url": url }), "kirc").is_ok() {
        return true;
    }
    match cdx_bus::app_path("browser") {
        Some(exe) => std::process::Command::new(exe).arg(url).spawn().is_ok(),
        None => false,
    }
}

/// Une notification pour le centre rapide du shell CDX (sans attendre : un fil s'en charge ; sans shell, elle se perd sans bruit).
pub fn notify_shell(title: &str, text: &str, level: &str) {
    let data = serde_json::json!({ "title": crate::text::truncate(title, 80), "text": crate::text::truncate(text, 400), "level": level });
    std::thread::spawn(move || {
        let _ = cdx_bus::send("shell", "notify", &data, "kirc");
    });
}

/// Surveille `suite.json` (toutes les 250 ms) et recolore les fenêtres quand l'accent change, sauf si un thème a été choisi à la main.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut w = cdx_bus::Watcher::new(&current());
        loop {
            std::thread::sleep(Duration::from_millis(250));
            if let Some(a) = w.changed() {
                *ACCENT.lock().unwrap_or_else(|e| e.into_inner()) = a.clone();
                let own = app.state::<App>().config.lock().map(|c| c.settings.theme.clone()).unwrap_or_else(|_| "cdx".into());
                if own == "cdx" {
                    let _ = app.emit("theme", to_kirc(&a));
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_is_cdx_and_the_others_keep_their_name() {
        assert_eq!(to_kirc("rouge"), "cdx");
        assert_eq!(to_kirc("bleu"), "bleu");
        for a in cdx_bus::ACCENTS {
            assert!(crate::config::THEMES.contains(&to_kirc(a).as_str()), "{a}");
        }
    }

    #[test]
    fn an_own_theme_wins_over_the_suite() {
        *ACCENT.lock().unwrap() = "bleu".into();
        assert_eq!(effective("cdx"), "bleu");
        assert_eq!(effective("ambre"), "ambre");
        *ACCENT.lock().unwrap() = "rouge".into();
        assert_eq!(effective("cdx"), "cdx");
    }
}
