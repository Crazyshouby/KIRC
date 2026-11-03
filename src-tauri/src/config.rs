//! Settings, address book and window memory, kept in %APPDATA%\CDX\Messages\config.json.
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
    /// The protocol of the account (see `chat::Protocol`); empty in entries saved before there were several.
    pub protocol: String,
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
            protocol: "irc".into(),
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
        let Some(protocol) = crate::chat::Protocol::parse(&self.protocol) else {
            return Err("Protocole inconnu".into());
        };
        if !protocol.needs_host() {
            return Ok(());
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

/// The themes the pages know (ui/cdx/themes.css, the sixteen KDX appearances of cdx-ui/tokens.json); "cdx" is the default look (the suite's "rouge").
pub const THEMES: [&str; 16] = ["cdx", "marine", "ambre", "jean", "tramway", "sarcelle", "chocolat", "lavande", "oxygene", "couchant", "techno", "reve-bleu", "reve-vert", "reve-violet", "lagon", "turquoise"];
/// Theme names of cdx-ui 1.x, mapped to today's names when an old config.json is read.
pub const THEME_ALIASES: [(&str, &str); 4] = [("bleu", "jean"), ("vert", "reve-vert"), ("violet", "reve-violet"), ("charbon", "techno")];

/// The presentations of a conversation of a messaging protocol (WhatsApp): "classique" is the IRC transcript, the
/// others are described in ui/messages.css.
pub const FEED_STYLES: [&str; 5] = ["classique", "aere", "blocs", "bulles", "cartes"];

/// Where our own messages stand in a chat presentation: "auto" (to the right in "bulles", to the left in the others).
pub const OWN_SIDES: [&str; 3] = ["auto", "gauche", "droite"];

/// The sizes of the text of the transcripts: the transcript font (13 px between lines), the interface font, and the
/// transcript font doubled (a pixel font stays sharp only at whole multiples).
pub const TEXT_SIZES: [&str; 3] = ["petite", "moyenne", "grande"];

/// Entries kept in each of the ignore and alert lists.
pub const MAX_LIST_ITEMS: usize = 200;

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
    /// Small (16 px) tiles and rows in the member lists, like CDX's "Use Small Icons In User Lists".
    pub small_member_icons: bool,
    /// Colours, bold and italic tell the kinds of lines of a transcript apart (off: all green, as in CDX).
    pub colored_chat: bool,
    /// How a conversation of WhatsApp (and the like) is laid out, one of `FEED_STYLES`. IRC windows keep the transcript.
    pub feed_style: String,
    /// The size of the text of every transcript, one of `TEXT_SIZES`.
    pub text_size: String,
    /// Which side our own messages stand on in those presentations, one of `OWN_SIDES`.
    pub own_side: String,
    /// Messages kept per window (50 to 5000).
    pub scrollback: u32,
    /// Open the server's console window when connecting.
    pub open_console: bool,
    /// Colour theme, one of `THEMES`.
    pub theme: String,
    /// Look for a newer release on GitHub at start and every six hours.
    pub check_updates: bool,
    /// Nicks or masks (`*`, `?`) whose messages are dropped, for every server.
    pub ignores: Vec<String>,
    /// Words that count as a mention besides one's own nick.
    pub alert_words: Vec<String>,
    /// Write conversations to %APPDATA%\CDX\Messages\logs and show the end of them when a window opens.
    pub log_enabled: bool,
    /// Log files older than this many days are deleted at start (0 = keep everything).
    pub log_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        let user = std::env::var("USERNAME").unwrap_or_default();
        let nick: String = user.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(16).collect();
        Settings {
            nick: if nick.is_empty() || nick.starts_with(|c: char| c.is_ascii_digit()) { "cdx_user".into() } else { nick },
            alt_nick: String::new(),
            username: String::new(),
            realname: "Messages".into(),
            quit_message: "Messages".into(),
            part_message: String::new(),
            notifications: true,
            open_queries: true,
            show_joins: true,
            show_timestamps: false,
            small_member_icons: true,
            colored_chat: true,
            feed_style: "blocs".into(),
            text_size: "petite".into(),
            own_side: "auto".into(),
            scrollback: 1000,
            open_console: true,
            theme: "cdx".into(),
            check_updates: true,
            ignores: Vec::new(),
            alert_words: Vec::new(),
            log_enabled: true,
            log_days: 90,
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
        if !THEMES.contains(&self.theme.as_str()) {
            return Err("Thème inconnu".into());
        }
        if !FEED_STYLES.contains(&self.feed_style.as_str()) {
            return Err("Présentation des conversations inconnue".into());
        }
        if !TEXT_SIZES.contains(&self.text_size.as_str()) {
            return Err("Taille du texte inconnue".into());
        }
        if !OWN_SIDES.contains(&self.own_side.as_str()) {
            return Err("Côté des messages inconnu".into());
        }
        if self.log_days > 3650 {
            return Err("La durée de conservation des journaux ne peut pas dépasser 3650 jours".into());
        }
        for (what, list) in [("pseudos ignorés", &self.ignores), ("mots d'alerte", &self.alert_words)] {
            if list.len() > MAX_LIST_ITEMS {
                return Err(format!("Trop de {what} (au plus {MAX_LIST_ITEMS})"));
            }
            if list.iter().any(|w| w.trim().is_empty() || w.chars().count() > 100 || w.chars().any(char::is_control)) {
                return Err(format!("Chaque entrée des {what} doit faire de 1 à 100 caractères, sans caractère de contrôle"));
            }
        }
        if self.ignores.iter().any(|w| w.chars().any(char::is_whitespace)) {
            return Err("Un pseudo ou un masque ignoré ne peut pas contenir d'espace".into());
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
    /// Chats hidden from the main menu's list, per server (folded names).
    pub hidden: HashMap<String, Vec<String>>,
}

const MAX_WINDOWS: usize = 300;
/// Chats hidden per server.
const MAX_HIDDEN: usize = 500;

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

    pub fn is_hidden(&self, server: &str, buffer: &str) -> bool {
        let key = crate::chat::fold(buffer);
        self.hidden.get(server).is_some_and(|v| v.contains(&key))
    }

    /// Hides a chat from the list of the main menu, or shows it again; true when something changed.
    pub fn set_hidden(&mut self, server: &str, buffer: &str, hide: bool) -> bool {
        let key = crate::chat::fold(buffer);
        if key.is_empty() || self.server(server).is_none() {
            return false;
        }
        let list = self.hidden.entry(server.to_string()).or_default();
        let at = list.iter().position(|b| *b == key);
        let changed = match (hide, at) {
            (true, None) if list.len() < MAX_HIDDEN => {
                list.push(key);
                true
            }
            (false, Some(i)) => {
                list.remove(i);
                true
            }
            _ => false,
        };
        if list.is_empty() {
            self.hidden.remove(server);
        }
        changed
    }

    pub fn remove_server(&mut self, id: &str) {
        self.hidden.remove(id);
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

/// %APPDATA%\CDX\Messages (or $CDX_MESSAGES_DATA_DIR, for tests). The folder of the former name, %APPDATA%\KIRC, is moved
/// there the first time it is found (config, logs, WhatsApp sessions: nothing to redo).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("CDX_MESSAGES_DATA_DIR") {
        return PathBuf::from(d);
    }
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = appdata.join("CDX").join("Messages");
    let legacy = appdata.join("KIRC");
    if !dir.exists() && legacy.is_dir() {
        let _ = std::fs::create_dir_all(appdata.join("CDX"));
        if let Err(e) = std::fs::rename(&legacy, &dir) {
            log::warn!("reprise du dossier de données de KIRC ({}) : {e}", legacy.display());
        }
    }
    dir
}

pub fn load(dir: &Path) -> Config {
    load_checked(dir).0
}

/// Un compte enregistré avant que Messages distingue les protocoles (0.4.0) n'a pas de champ `protocol` : il est lu comme IRC. Un compte WhatsApp de cette
/// époque se reconnaît à son adresse vide et à sa session `<data>/whatsapp/<id>.db` : il reprend son protocole — sinon Messages tenterait de joindre un serveur IRC
/// à l'adresse vide, c'est-à-dire la machine elle-même, port 6697, et afficherait « connexion refusée » (os error 10061) à chaque démarrage.
#[cfg(feature = "whatsapp")]
fn migrate_protocols(dir: &Path, c: &mut Config) {
    for s in &mut c.servers {
        if s.host.trim().is_empty() && (s.protocol.is_empty() || s.protocol == "irc") && dir.join("whatsapp").join(format!("{}.db", s.id)).is_file() {
            log::info!("compte « {} » : compte WhatsApp d'avant les protocoles, protocole retrouvé", s.name);
            s.protocol = "whatsapp".into();
        }
    }
}
#[cfg(not(feature = "whatsapp"))]
fn migrate_protocols(_dir: &Path, _c: &mut Config) {}

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
                if c.settings.theme == "kdx" { c.settings.theme = "cdx".into(); }   // le look par défaut s'appelait « kdx » (suite KDX, jusqu'au 31/10/2026)
                if let Some((_, t)) = THEME_ALIASES.iter().find(|(a, _)| *a == c.settings.theme) { c.settings.theme = (*t).into(); }   // cdx-ui 1.x : bleu, vert, violet, charbon
                migrate_protocols(dir, &mut c);
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

/// Writes the file atomically: a crash in the middle never leaves it truncated, and two callers
/// never write the same temporary file at once (their bytes would mix).
pub fn save(dir: &Path, cfg: &Config) -> io::Result<()> {
    static SAVING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let text = serde_json::to_string_pretty(cfg).map_err(io::Error::other)?;
    let _g = SAVING.lock().unwrap_or_else(|e| e.into_inner());
    fs::create_dir_all(dir)?;
    write_atomic(&dir.join("config.json"), text.as_bytes())
}

/// Temporary file next to `path`, flushed to the disk, then renamed over it; the temporary file
/// is removed if anything fails. Callers serialize their own writes to the same path.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let res = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, host: &str) -> ServerEntry {
        ServerEntry { name: name.into(), host: host.into(), ..Default::default() }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("messages-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_server_saved_before_protocols_existed_is_irc() {
        let old: ServerEntry = serde_json::from_str(r#"{"id":"s1","name":"Ancien","host":"irc.example.org","port":6697}"#).unwrap();
        assert_eq!(old.protocol, "irc");
        assert!(old.validate().is_ok());
        let blank = ServerEntry { protocol: String::new(), ..old };
        assert!(blank.validate().is_ok(), "an empty protocol means IRC");
    }

    #[test]
    #[cfg(feature = "whatsapp")]
    fn an_old_whatsapp_account_without_protocol_gets_it_back() {
        let d = std::env::temp_dir().join(format!("messages-test-migrate-wa-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("whatsapp")).unwrap();
        std::fs::write(d.join("whatsapp").join("sabc.db"), "x").unwrap();
        std::fs::write(d.join("config.json"), r#"{"version":1,"servers":[{"id":"sabc","name":"WhatsApp","host":"","port":6697,"autoconnect":true},{"id":"sirc","name":"Libera","host":"irc.libera.chat","port":6697},{"id":"snew","name":"Neuf","host":"","port":6697}]}"#).unwrap();
        let (c, ok) = load_checked(&d);
        assert!(ok);
        assert_eq!(c.servers.iter().find(|s| s.id == "sabc").unwrap().protocol, "whatsapp", "an old WhatsApp account (empty host, session file) gets its protocol back");
        assert_eq!(c.servers.iter().find(|s| s.id == "sirc").unwrap().protocol, "irc");
        assert_eq!(c.servers.iter().find(|s| s.id == "snew").unwrap().protocol, "irc", "an empty host without a session file stays as it is");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unknown_protocol_is_refused() {
        let mut e = entry("a", "h");
        e.protocol = "telegram".into();
        assert_eq!(e.validate(), Err("Protocole inconnu".to_string()));
    }

    #[test]
    fn a_chat_can_be_hidden_and_shown_again() {
        let mut c = Config::default();
        let id = c.upsert_server(entry("a", "irc.example.org")).unwrap();
        assert!(!c.is_hidden(&id, "#Rust"));
        assert!(c.set_hidden(&id, "#Rust", true));
        assert!(c.is_hidden(&id, "#rust"), "the case does not matter");
        assert!(!c.set_hidden(&id, "#RUST", true), "already hidden: nothing changes");
        assert!(!c.is_hidden("other", "#rust"));
        assert!(c.set_hidden(&id, "#rust", false));
        assert!(!c.is_hidden(&id, "#Rust"));
        assert!(c.hidden.is_empty(), "an empty list is not kept");
        assert!(!c.set_hidden(&id, "#rust", false), "not hidden: nothing to do");
        assert!(!c.set_hidden("unknown", "#rust", true), "a server that does not exist");
    }

    #[test]
    fn removing_a_server_forgets_what_was_hidden() {
        let mut c = Config::default();
        let id = c.upsert_server(entry("a", "irc.example.org")).unwrap();
        c.set_hidden(&id, "#a", true);
        c.remove_server(&id);
        assert!(c.hidden.is_empty());
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
    fn the_presentation_and_the_text_size_are_among_the_known_ones() {
        for f in FEED_STYLES {
            assert!(Settings { feed_style: f.into(), ..Default::default() }.validate().is_ok(), "{f}");
        }
        for t in TEXT_SIZES {
            assert!(Settings { text_size: t.into(), ..Default::default() }.validate().is_ok(), "{t}");
        }
        assert!(Settings { feed_style: "bulle".into(), ..Default::default() }.validate().is_err());
        assert!(Settings { text_size: "énorme".into(), ..Default::default() }.validate().is_err());
        for o in OWN_SIDES {
            assert!(Settings { own_side: o.into(), ..Default::default() }.validate().is_ok(), "{o}");
        }
        assert!(Settings { own_side: "milieu".into(), ..Default::default() }.validate().is_err());
        // a config saved before these settings existed gets the defaults
        let s: Settings = serde_json::from_str(r#"{"nick":"x"}"#).unwrap();
        assert_eq!((s.feed_style.as_str(), s.text_size.as_str()), ("blocs", "petite"));
        assert_eq!(s.own_side, "auto");
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
        let s = Settings { ignores: vec!["a b".into()], ..Default::default() };
        assert!(s.validate().is_err(), "a mask has no space");
        let s = Settings { alert_words: vec!["deux mots".into()], ..Default::default() };
        assert!(s.validate().is_ok());
        let s = Settings { alert_words: vec![String::new()], ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { ignores: vec!["x".into(); MAX_LIST_ITEMS + 1], ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { log_days: 9999, ..Default::default() };
        assert!(s.validate().is_err());
        for t in THEMES {
            assert!(Settings { theme: t.into(), ..Default::default() }.validate().is_ok(), "{t}");
        }
        assert!(Settings { theme: "../x".into(), ..Default::default() }.validate().is_err());
        // A file written before themes existed has none: it gets the default.
        let old: Settings = serde_json::from_str(r#"{"nick":"a"}"#).unwrap();
        assert_eq!(old.theme, "cdx");
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
    fn concurrent_saves_never_mix_their_bytes() {
        let dir = tmp("concurrent");
        let names: Vec<String> = (0..8).map(|i| format!("pseudo{i}-{}", "x".repeat(4000))).collect();
        std::thread::scope(|s| {
            for n in &names {
                let dir = &dir;
                s.spawn(move || {
                    let mut c = Config { version: 1, ..Default::default() };
                    c.settings.nick = n.clone();
                    for _ in 0..20 {
                        save(dir, &c).unwrap();
                    }
                });
            }
        });
        let text = fs::read_to_string(dir.join("config.json")).unwrap();
        let back: Config = serde_json::from_str(&text).expect("config.json lisible après des écritures simultanées");
        assert!(names.contains(&back.settings.nick));
        assert!(!dir.join("config.json.tmp").exists());
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
