//! cdx_bus : le client Rust des réglages communs et du bus de la suite CDX (voir docs/SUITE.md). Un seul fichier, à copier tel quel
//! dans une application (`node tools/charte-sync.mjs --write <application>`) ; il n'a besoin que de `serde_json`.
//!
//! * [`read_accent`], [`Watcher`] : l'accent de `%APPDATA%\CDX\suite.json`, relu à chaud (sondage : on appelle [`Watcher::changed`] toutes les 250 ms) ;
//! * [`send`] : un message « tire et oublie » vers le tube `\\.\pipe\cdx.<app>` ;
//! * [`compose`] : la ligne JSON d'un message.
//!
//! Les variables d'environnement `CDX_SUITE_DIR` et `CDX_PIPE_PREFIX` ne servent qu'aux essais : elles évitent de toucher aux vrais réglages et au vrai bus.

use serde_json::{json, Value};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Longueur maximale d'une ligne du bus (64 Ko).
pub const MAX_LINE: usize = 65536;

/// Les accents de la suite : les seize thèmes de `tokens.json` (les apparences de KDX 1.6).
pub const ACCENTS: [&str; 16] = ["rouge", "marine", "ambre", "jean", "tramway", "sarcelle", "chocolat", "lavande", "oxygene", "couchant", "techno", "reve-bleu", "reve-vert", "reve-violet", "lagon", "turquoise"];
/// Les anciens noms (cdx-ui 1.x) et le thème qu'ils désignent aujourd'hui.
pub const ACCENT_ALIASES: [(&str, &str); 5] = [("bleu", "jean"), ("vert", "reve-vert"), ("violet", "reve-violet"), ("charbon", "techno"), ("cdx", "rouge")];

/// Le nom actuel d'un accent (alias compris), ou `None`.
pub fn canonical_accent(s: &str) -> Option<&'static str> {
    ACCENTS.iter().copied().find(|a| *a == s).or_else(|| ACCENT_ALIASES.iter().find(|(a, _)| *a == s).map(|(_, b)| *b))
}

/// Les applications qui peuvent recevoir (un tube chacune).
pub const APPS: [&str; 5] = ["shell", "explorer", "browser", "audio_linker", "messages"];

pub fn is_accent(s: &str) -> bool {
    canonical_accent(s).is_some()
}

pub fn is_app(s: &str) -> bool {
    APPS.contains(&s)
}

/// Le dossier des réglages : `CDX_SUITE_DIR` (essais), sinon `%APPDATA%\CDX`.
pub fn suite_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("CDX_SUITE_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("CDX"))
}

pub fn suite_path() -> Option<PathBuf> {
    suite_dir().map(|d| d.join("suite.json"))
}

/// L'accent d'un contenu de `suite.json` : celui qu'il donne s'il est connu et si la version est 1 (ou absente), sinon `fallback`.
pub fn parse_accent(json: &str, fallback: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return fallback.to_string() };
    if !v.is_object() {
        return fallback.to_string();
    }
    if let Some(ver) = v.get("version") {
        if ver.as_i64() != Some(1) {
            return fallback.to_string();
        }
    }
    match v.get("accent").and_then(Value::as_str).and_then(canonical_accent) {
        Some(a) => a.to_string(),
        None => fallback.to_string(),
    }
}

/// L'accent écrit dans `suite.json`, ou `fallback` (fichier absent, abîmé, d'une autre version, accent inconnu) : jamais d'erreur.
pub fn read_accent(fallback: &str) -> String {
    match suite_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(t) => parse_accent(&t, fallback),
        None => fallback.to_string(),
    }
}

/// Surveille `suite.json` : à appeler toutes les 250 ms.
pub struct Watcher {
    current: String,
    stamp: Option<(SystemTime, u64)>,
}

impl Watcher {
    pub fn new(current: &str) -> Self {
        let mut w = Watcher { current: current.to_string(), stamp: None };
        w.stamp = w.read_stamp();
        w
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    fn read_stamp(&self) -> Option<(SystemTime, u64)> {
        let m = std::fs::metadata(suite_path()?).ok()?;
        Some((m.modified().ok()?, m.len()))
    }

    /// Le nouvel accent, quand le fichier a changé et que l'accent n'est plus le même.
    pub fn changed(&mut self) -> Option<String> {
        let s = self.read_stamp();
        if s == self.stamp {
            return None;
        }
        self.stamp = s;
        let a = read_accent(&self.current);
        if a == self.current {
            return None;
        }
        self.current = a.clone();
        Some(a)
    }
}

/// Le chemin de l'exécutable d'une application de la suite, d'après `apps.json` (chaque programme y écrit le sien au démarrage) ; `None` si le nom n'est pas
/// dans la liste des applications, si le fichier manque ou si l'exécutable n'existe plus.
pub fn app_path(app: &str) -> Option<PathBuf> {
    if !is_app(app) {
        return None;
    }
    let text = std::fs::read_to_string(suite_dir()?.join("apps.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let p = PathBuf::from(v.get(app)?.as_str()?);
    p.is_file().then_some(p)
}

/// La ligne d'un message : `{"type":…,"from":…,"data":…}` (sans saut de ligne final).
pub fn compose(typ: &str, from: &str, data: &Value) -> String {
    json!({ "type": typ, "from": from, "data": data }).to_string()
}

fn pipe_path(app: &str) -> String {
    let prefix = std::env::var("CDX_PIPE_PREFIX").ok().filter(|p| !p.is_empty()).unwrap_or_else(|| "cdx.".to_string());
    format!(r"\\.\pipe\{prefix}{app}")
}

/// Envoie un message « tire et oublie » à `app`. Erreur si le nom est inconnu, si la ligne dépasse 64 Ko ou si l'application n'écoute pas.
pub fn send(app: &str, typ: &str, data: &Value, from: &str) -> io::Result<()> {
    if !is_app(app) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "application inconnue"));
    }
    let line = compose(typ, from, data) + "\n";
    if line.len() > MAX_LINE {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "ligne de plus de 64 Ko"));
    }
    let path = pipe_path(app);
    // Tube occupé (231) : on réessaie un court instant ; absent : l'application n'est pas lancée.
    let mut last = None;
    for _ in 0..10 {
        match std::fs::OpenOptions::new().write(true).open(&path) {
            Ok(mut f) => {
                f.write_all(line.as_bytes())?;
                return f.flush();
            }
            Err(e) if e.raw_os_error() == Some(231) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "tube occupé")))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Les variables d'environnement sont communes à tout le processus : les essais qui les changent passent l'un après l'autre.
    static ENV: Mutex<()> = Mutex::new(());

    #[test]
    fn compose_builds_the_envelope() {
        let l = compose("open-url", "messages", &json!({ "url": "https://example.org/" }));
        let v: Value = serde_json::from_str(&l).unwrap();
        assert_eq!(v["type"], "open-url");
        assert_eq!(v["from"], "messages");
        assert_eq!(v["data"]["url"], "https://example.org/");
        assert!(!l.contains('\n'));
    }

    #[test]
    fn parse_accent_cases() {
        assert_eq!(parse_accent(r#"{"version":1,"accent":"jean"}"#, "rouge"), "jean");
        assert_eq!(parse_accent(r#"{"accent":"reve-vert"}"#, "rouge"), "reve-vert");
        // les anciens noms (charte < 2.0.0) sont lus sous leur nom canonique
        assert_eq!(parse_accent(r#"{"version":1,"accent":"bleu"}"#, "rouge"), "jean");
        assert_eq!(parse_accent(r#"{"accent":"charbon"}"#, "rouge"), "techno");
        assert_eq!(parse_accent(r#"{"version":2,"accent":"jean"}"#, "ambre"), "ambre");
        assert_eq!(parse_accent(r#"{"version":"1","accent":"jean"}"#, "ambre"), "ambre");
        assert_eq!(parse_accent(r#"{"version":1,"accent":"../x"}"#, "lavande"), "lavande");
        assert_eq!(parse_accent("{{{ pas du json", "techno"), "techno");
        assert_eq!(parse_accent("[1,2]", "rouge"), "rouge");
        assert_eq!(parse_accent("", "rouge"), "rouge");
    }

    #[test]
    fn app_path_reads_apps_json() {
        let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("cdx-bus-apps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("CDX_SUITE_DIR", &dir);
        assert_eq!(app_path("browser"), None, "fichier absent");
        let exe = std::env::current_exe().unwrap();
        let text = json!({ "browser": exe, "explorer": "C:\\n\\existe\\pas.exe", "evil": exe }).to_string();
        std::fs::write(dir.join("apps.json"), text).unwrap();
        assert_eq!(app_path("browser"), Some(exe));
        assert_eq!(app_path("explorer"), None, "exécutable disparu");
        assert_eq!(app_path("evil"), None, "nom hors liste");
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("CDX_SUITE_DIR");
    }

    #[test]
    fn app_names_are_a_whitelist() {
        assert!(is_app("browser") && is_app("audio_linker"));
        assert!(!is_app("..\\evil") && !is_app("") && !is_app("Browser"));
        assert!(is_accent("sarcelle") && !is_accent("rose"));
    }

    #[test]
    fn watcher_follows_the_file() {
        let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("cdx-bus-rs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("CDX_SUITE_DIR", &dir);
        assert_eq!(read_accent("sarcelle"), "sarcelle", "fichier absent");
        let mut w = Watcher::new("rouge");
        assert_eq!(w.changed(), None);
        std::fs::write(suite_path().unwrap(), r#"{"version":1,"accent":"bleu"}"#).unwrap();
        assert_eq!(w.changed().as_deref(), Some("jean"), "l'ancien nom « bleu » est signalé sous son nom canonique");
        assert_eq!(w.changed(), None, "pas de second signal");
        std::thread::sleep(Duration::from_millis(30));
        std::fs::write(suite_path().unwrap(), r#"{"version":1,"accent":"jean"}"#).unwrap();
        assert_eq!(w.changed(), None, "réécrit sous l'autre nom du même accent : rien ne change");
        std::fs::write(suite_path().unwrap(), "n'importe quoi").unwrap();
        assert_eq!(w.changed(), None, "fichier abîmé : l'accent précédent est gardé");
        assert_eq!(w.current(), "jean");
        std::fs::write(suite_path().unwrap(), r#"{"version":1,"accent":"ambre"}"#).unwrap();
        assert_eq!(w.changed().as_deref(), Some("ambre"));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("CDX_SUITE_DIR");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_reaches_a_listening_pipe() {
        use tokio::io::AsyncReadExt;
        use tokio::net::windows::named_pipe::ServerOptions;
        let prefix = format!("cdxtest{}.", std::process::id());
        {
            let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
            std::env::set_var("CDX_PIPE_PREFIX", &prefix);
        }
        assert!(send("browser", "open-url", &json!({}), "messages").is_err(), "personne n'écoute");
        assert!(send("nimporte", "x", &json!({}), "messages").is_err(), "nom inconnu");
        let mut server = ServerOptions::new().first_pipe_instance(true).access_outbound(false).create(format!(r"\\.\pipe\{prefix}browser")).unwrap();
        let reader = tokio::spawn(async move {
            server.connect().await.unwrap();
            let mut got = String::new();
            server.read_to_string(&mut got).await.unwrap();
            got
        });
        tokio::task::spawn_blocking(|| send("browser", "open-url", &json!({ "url": "https://example.org/" }), "messages").unwrap()).await.unwrap();
        let got = reader.await.unwrap();
        assert!(got.ends_with('\n') && got.matches('\n').count() == 1, "une seule ligne");
        let v: Value = serde_json::from_str(got.trim_end()).unwrap();
        assert_eq!((v["type"].as_str(), v["from"].as_str(), v["data"]["url"].as_str()), (Some("open-url"), Some("messages"), Some("https://example.org/")));
        let huge = json!({ "u": "x".repeat(MAX_LINE) });
        assert!(send("browser", "open-url", &huge, "messages").is_err(), "plus de 64 Ko");
        std::env::remove_var("CDX_PIPE_PREFIX");
    }
}
