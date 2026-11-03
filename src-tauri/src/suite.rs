//! L'accent de la suite CDX (`%APPDATA%\CDX\suite.json`, voir cdx-ui/docs/SUITE.md).
//!
//! Messages suit l'accent de la suite tant que son réglage « thème » est sur sa valeur par défaut (`cdx`) ; un thème choisi à la main reste prioritaire.
//! Chez Messages le rouge de la suite s'appelle `cdx` (le look par défaut) ; les autres accents portent le même nom des deux côtés.

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

/// Le nom, chez Messages, d'un accent de la suite.
pub fn to_app_theme(accent: &str) -> String {
    if accent == "rouge" { "cdx".into() } else { accent.to_string() }
}

/// Le thème que les fenêtres doivent porter : celui du réglage, ou l'accent de la suite si le réglage est sur `cdx`.
pub fn effective(setting: &str) -> String {
    if setting == "cdx" { to_app_theme(&current()) } else { setting.to_string() }
}

/// Ouvre un lien dans le navigateur CDX : par le bus s'il tourne, sinon en le lançant s'il est installé (apps.json). `false` : l'appelant se rabat sur
/// l'ouvreur du système. Seuls les liens http et https arrivent ici (voir `commands::open_url`).
pub fn open_in_cdx_browser(url: &str) -> bool {
    if cdx_bus::send("browser", "open-url", &serde_json::json!({ "url": url }), "messages").is_ok() {
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
        let _ = cdx_bus::send("shell", "notify", &data, "messages");
    });
}

#[cfg(windows)]
mod change {
    //! Les notifications de changement d'un dossier (kernel32), sans dépendance de plus.
    #[link(name = "kernel32")]
    extern "system" {
        fn FindFirstChangeNotificationW(path: *const u16, subtree: i32, filter: u32) -> isize;
        fn FindNextChangeNotification(h: isize) -> i32;
        fn FindCloseChangeNotification(h: isize) -> i32;
        fn WaitForSingleObject(h: isize, ms: u32) -> u32;
    }
    pub struct Dir(isize);
    impl Dir {
        /// Noms de fichiers et dates d'écriture du dossier (l'écriture atomique de suite.json est un renommage).
        pub fn watch(dir: &std::path::Path) -> Option<Dir> {
            use std::os::windows::ffi::OsStrExt;
            let w: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
            let h = unsafe { FindFirstChangeNotificationW(w.as_ptr(), 0, 0x1 | 0x10) };
            if h == -1 || h == 0 { None } else { Some(Dir(h)) }
        }
        /// Attend un changement (true) ou la fin du délai (false), puis réarme.
        pub fn wait(&self, ms: u32) -> bool {
            let hit = unsafe { WaitForSingleObject(self.0, ms) } == 0;
            if hit { unsafe { FindNextChangeNotification(self.0) }; }
            hit
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) { unsafe { FindCloseChangeNotification(self.0) }; }
    }
}

/// Surveille `suite.json` et recolore les fenêtres quand l'accent change, sauf si un thème a été choisi à la main. Par notification de changement du
/// dossier (et une relecture de secours toutes les 5 s) ; avant : un sondage toutes les 250 ms, sans fin, pour un fichier qui change quelques fois par mois.
/// Sans notification possible (dossier absent…), retour au sondage de 250 ms.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut w = cdx_bus::Watcher::new(&current());
        #[cfg(windows)]
        let dir = cdx_bus::suite_dir().and_then(|d| change::Dir::watch(&d));
        loop {
            #[cfg(windows)]
            match &dir {
                Some(d) => { d.wait(5000); }
                None => std::thread::sleep(Duration::from_millis(250)),
            }
            #[cfg(not(windows))]
            std::thread::sleep(Duration::from_millis(250));
            if let Some(a) = w.changed() {
                *ACCENT.lock().unwrap_or_else(|e| e.into_inner()) = a.clone();
                let own = app.state::<App>().config.lock().map(|c| c.settings.theme.clone()).unwrap_or_else(|_| "cdx".into());
                if own == "cdx" {
                    let _ = app.emit("theme", to_app_theme(&a));
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn a_write_in_the_watched_folder_wakes_the_watcher() {
        let dir = std::env::temp_dir().join(format!("messages-suite-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let w = change::Dir::watch(&dir).expect("surveillance du dossier");
        assert!(!w.wait(50), "rien n'a changé : pas de réveil");
        let d2 = dir.clone();
        let writer = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(100)); std::fs::write(d2.join("suite.json"), "{}").unwrap(); });
        assert!(w.wait(3000), "l'écriture réveille l'attente");
        writer.join().unwrap();
        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn red_is_cdx_and_the_others_keep_their_name() {
        assert_eq!(to_app_theme("rouge"), "cdx");
        assert_eq!(to_app_theme("jean"), "jean");
        for a in cdx_bus::ACCENTS {
            assert!(crate::config::THEMES.contains(&to_app_theme(a).as_str()), "{a}");
        }
    }

    #[test]
    fn an_own_theme_wins_over_the_suite() {
        *ACCENT.lock().unwrap() = "jean".into();
        assert_eq!(effective("cdx"), "jean");
        assert_eq!(effective("ambre"), "ambre");
        *ACCENT.lock().unwrap() = "rouge".into();
        assert_eq!(effective("cdx"), "cdx");
    }
}
