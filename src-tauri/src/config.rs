//! Settings, address book and window memory, kept in %APPDATA%\KIRC\config.json.
//! Passwords never go in this file: they live in the Windows credential manager.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ServerEntry {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    /// Accept a certificate that does not validate (self-signed server).
    pub accept_invalid_certs: bool,
    /// Empty = the default of the settings.
    pub nick: String,
    pub alt_nick: String,
    pub username: String,
    pub realname: String,
    /// "none", "sasl" (account + password) or "nickserv" (password).
    pub auth: String,
    /// SASL account; empty = the nick.
    pub account: String,
    pub autoconnect: bool,
    /// "#channel" or "#channel key", separated by commas or new lines.
    pub autojoin: String,
}

impl Default for ServerEntry {
    fn default() -> Self {
        ServerEntry {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: 6697,
            tls: true,
            accept_invalid_certs: false,
            nick: String::new(),
            alt_nick: String::new(),
            username: String::new(),
            realname: String::new(),
            auth: "none".into(),
            account: String::new(),
            autoconnect: false,
            autojoin: String::new(),
        }
    }
}

impl ServerEntry {
    /// The channels to join after connecting, one "#channel[ key]" per item.
    pub fn autojoin_list(&self) -> Vec<String> {
        self.autojoin
            .split([',', '\n', ';'])
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Checks what the user typed; the message is shown as is.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Donnez un nom à ce serveur".into());
        }
        let host = self.host.trim();
        if host.is_empty() {
            return Err("L'adresse du serveur est vide".into());
        }
        if host.chars().any(|c| c.is_whitespace() || c == '/' || c == '\\' || c == '@' || c == ':' && !host.contains('[')) {
            return Err("L'adresse du serveur ne doit contenir ni espace, ni « / », ni « : » (le port se règle à part)".into());
        }
        if self.port == 0 {
            return Err("Le port doit être compris entre 1 et 65535".into());
        }
        for field in [&self.nick, &self.alt_nick, &self.username] {
            if field.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err("Les pseudos et l'identifiant ne peuvent pas contenir d'espace".into());
            }
        }
        if !matches!(self.auth.as_str(), "none" | "sasl" | "nickserv") {
            return Err("Mode d'authentification inconnu".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub nick: String,
    pub alt_nick: String,
    pub username: String,
    pub realname: String,
    pub quit_message: String,
    pub part_message: String,
    /// Desktop notifications for private messages and mentions.
    pub notifications: bool,
    /// Open a window for each private conversation that starts.
    pub open_queries: bool,
    /// Join, part, quit, nick lines in the channels.
    pub show_joins: bool,
    pub show_timestamps: bool,
    /// Messages kept per window (50 to 5000).
    pub scrollback: u32,
    /// Open the server's console window when connecting.
    pub open_console: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let user = std::env::var("USERNAME").unwrap_or_default();
        let nick: String = user.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(16).collect();
        Settings {
            nick: if nick.is_empty() || nick.starts_with(|c: char| c.is_ascii_digit()) { "kirc_user".into() } else { nick },
            alt_nick: String::new(),
            username: String::new(),
            realname: "KIRC".into(),
            quit_message: "KIRC".into(),
            part_message: String::new(),
            notifications: true,
            open_queries: true,
            show_joins: true,
            show_timestamps: true,
            scrollback: 1000,
            open_console: true,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.nick.trim().is_empty() {
            return Err("Le pseudo par défaut ne peut pas être vide".into());
        }
        for f in [&self.nick, &self.alt_nick, &self.username] {
            if f.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err("Les pseudos et l'identifiant ne peuvent pas contenir d'espace".into());
            }
        }
        if !(50..=5000).contains(&self.scrollback) {
            return Err("L'historique doit être compris entre 50 et 5000 lignes".into());
        }
        Ok(())
    }
}

/// Position and size of a window, in physical pixels.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub settings: Settings,
    pub servers: Vec<ServerEntry>,
    pub windows: HashMap<String, Geometry>,
    /// Windows other than chats that were open at exit and come back at start.
    pub open_windows: Vec<String>,
}

const MAX_WINDOWS: usize = 300;

impl Config {
    pub fn server(&self, id: &str) -> Option<&ServerEntry> {
        self.servers.iter().find(|s| s.id == id)
    }

    /// Adds or replaces a server (by id); a new id is made if the entry has none.
    pub fn upsert_server(&mut self, mut entry: ServerEntry) -> Result<String, String> {
        entry.name = entry.name.trim().to_string();
        entry.host = entry.host.trim().to_string();
        entry.validate()?;
        if entry.id.is_empty() {
            entry.id = new_id(&self.servers);
        }
        let id = entry.id.clone();
        match self.servers.iter_mut().find(|s| s.id == id) {
            Some(slot) => *slot = entry,
            None => self.servers.push(entry),
        }
        Ok(id)
    }

    pub fn remove_server(&mut self, id: &str) {
        self.servers.retain(|s| s.id != id);
        let prefix_a = format!("chat:{id}:");
        let console = format!("console:{id}");
        self.windows.retain(|k, _| !k.starts_with(&prefix_a) && *k != console);
    }

    pub fn remember_window(&mut self, key: &str, g: Geometry) {
        if self.windows.len() >= MAX_WINDOWS && !self.windows.contains_key(key) {
            // Forget an arbitrary old chat window rather than grow without bound.
            if let Some(old) = self.windows.keys().find(|k| k.starts_with("chat:")).cloned() {
                self.windows.remove(&old);
            }
        }
        self.windows.insert(key.to_string(), g);
    }
}

fn new_id(existing: &[ServerEntry]) -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let mut n = (t as u64) & 0xffff_ffff;
    loop {
        let id = format!("s{n:x}");
        if !existing.iter().any(|s| s.id == id) {
            return id;
        }
        n += 1;
    }
}

/// %APPDATA%\KIRC (or $KIRC_DATA_DIR, for tests).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("KIRC_DATA_DIR") {
        return PathBuf::from(d);
    }
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("KIRC")
}

pub fn load(dir: &Path) -> Config {
    load_checked(dir).0
}

/// The configuration, and whether it may be written back. A file that exists but could not be read
/// (locked by an antivirus, no rights) leaves the defaults in memory: writing them over the real
/// address book would destroy it, so in that case nothing is saved this session.
pub fn load_checked(dir: &Path) -> (Config, bool) {
    let path = dir.join("config.json");
    let fresh = || Config { version: 1, ..Default::default() };
    match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Config>(&text) {
            Ok(mut c) => {
                c.version = 1;
                (c, true)
            }
            Err(e) => {
                log::error!("config.json illisible ({e}) : conservé en config.json.bad");
                let moved = fs::rename(&path, dir.join("config.json.bad")).is_ok();
                (fresh(), moved)
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => (fresh(), true),
        Err(e) => {
            log::error!("config.json non lu : {e}");
            (fresh(), false)
        }
    }
}

/// Writes the file atomically: a crash in the middle never leaves it truncated.
pub fn save(dir: &Path, cfg: &Config) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join("config.json.tmp");
    let text = serde_json::to_string_pretty(cfg).map_err(io::Error::other)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, dir.join("config.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, host: &str) -> ServerEntry {
        ServerEntry { name: name.into(), host: host.into(), ..Default::default() }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kirc-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn autojoin_accepts_commas_lines_and_keys() {
        let mut e = entry("a", "h");
        e.autojoin = "#a, #b  clé\n#c;  ".into();
        assert_eq!(e.autojoin_list(), vec!["#a", "#b clé", "#c"]);
        e.autojoin = "".into();
        assert!(e.autojoin_list().is_empty());
    }

    #[test]
    fn validation_catches_typical_mistakes() {
        assert!(entry("Libera", "irc.libera.chat").validate().is_ok());
        assert!(entry("", "irc.libera.chat").validate().is_err());
        assert!(entry("x", "").validate().is_err());
        assert!(entry("x", "irc.libera.chat:6697").validate().is_err(), "port belongs in its own field");
        assert!(entry("x", "ircs://irc.libera.chat").validate().is_err());
        assert!(entry("x", "a b").validate().is_err());
        assert!(entry("x", "[::1]").validate().is_ok());
        let mut e = entry("x", "h");
        e.port = 0;
        assert!(e.validate().is_err());
        let mut e = entry("x", "h");
        e.nick = "bad nick".into();
        assert!(e.validate().is_err());
        let mut e = entry("x", "h");
        e.auth = "kerberos".into();
        assert!(e.validate().is_err());
    }

    #[test]
    fn settings_validation() {
        assert!(Settings::default().validate().is_ok());
        let s = Settings { nick: " ".into(), ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { scrollback: 10, ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { alt_nick: "a b".into(), ..Default::default() };
        assert!(s.validate().is_err());
    }

    #[test]
    fn upsert_creates_unique_ids_and_replaces() {
        let mut c = Config::default();
        let a = c.upsert_server(entry("A", "a.example")).unwrap();
        let b = c.upsert_server(entry("B", "b.example")).unwrap();
        assert_ne!(a, b);
        let mut again = c.server(&a).unwrap().clone();
        again.name = "A2".into();
        assert_eq!(c.upsert_server(again).unwrap(), a);
        assert_eq!(c.servers.len(), 2);
        assert_eq!(c.server(&a).unwrap().name, "A2");
        assert!(c.upsert_server(entry("", "x")).is_err());
        assert_eq!(c.servers.len(), 2);
    }

    #[test]
    fn removing_a_server_forgets_its_windows() {
        let mut c = Config::default();
        let id = c.upsert_server(entry("A", "a.example")).unwrap();
        let g = Geometry { x: 1, y: 2, w: 3, h: 4 };
        c.remember_window(&format!("chat:{id}:#a"), g);
        c.remember_window(&format!("console:{id}"), g);
        c.remember_window("launcher", g);
        c.remove_server(&id);
        assert_eq!(c.windows.keys().collect::<Vec<_>>(), vec!["launcher"]);
        assert!(c.servers.is_empty());
    }

    #[test]
    fn window_memory_is_bounded() {
        let mut c = Config::default();
        let g = Geometry { x: 0, y: 0, w: 1, h: 1 };
        for i in 0..(MAX_WINDOWS + 50) {
            c.remember_window(&format!("chat:s:#{i}"), g);
        }
        assert!(c.windows.len() <= MAX_WINDOWS);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tmp("roundtrip");
        let mut c = Config { version: 1, ..Default::default() };
        let id = c.upsert_server(entry("Libera", "irc.libera.chat")).unwrap();
        c.settings.nick = "valentin".into();
        c.remember_window("launcher", Geometry { x: 10, y: 20, w: 200, h: 400 });
        save(&dir, &c).unwrap();
        assert!(!dir.join("config.json.tmp").exists());
        let back = load(&dir);
        assert_eq!(back.settings.nick, "valentin");
        assert_eq!(back.server(&id).unwrap().host, "irc.libera.chat");
        assert_eq!(back.windows["launcher"], Geometry { x: 10, y: 20, w: 200, h: 400 });
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_fields_take_defaults_and_a_corrupt_file_is_set_aside() {
        let dir = tmp("corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.json"), r#"{"servers":[{"name":"X","host":"h"}]}"#).unwrap();
        let c = load(&dir);
        assert_eq!(c.servers[0].port, 6697);
        assert!(c.servers[0].tls);
        fs::write(dir.join("config.json"), "{ pas du json").unwrap();
        let c = load(&dir);
        assert!(c.servers.is_empty());
        assert!(dir.join("config.json.bad").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_password_field_exists_in_the_file() {
        let mut c = Config::default();
        c.upsert_server(entry("A", "a.example")).unwrap();
        let text = serde_json::to_string(&c).unwrap().to_lowercase();
        assert!(!text.contains("password") && !text.contains("secret"));
    }
}
