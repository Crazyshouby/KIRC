//! Conversation logs: one text file per channel or private conversation, in
//! %APPDATA%\KIRC\logs\<server>\<buffer>.log, one line per message:
//!
//!   2026-10-28 21:41:03 <nick> text          message
//!   2026-10-28 21:41:09 * nick text          action
//!   2026-10-28 21:41:12 -nick- text          notice
//!   2026-10-28 21:41:15 *** text             join, part, quit, kick, nick, mode, topic
//!
//! Writing happens on its own thread, so a slow disk never slows the network.
//! The tail of a file is read back when a window opens (see `tail`).

use crate::irc::event::{Line, LineKind};
use crate::irc::session::fold;
use crate::text;
use chrono::{Local, NaiveDateTime, TimeZone};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, SystemTime};

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
/// How much of the end of a file is read back at most.
const TAIL_BYTES: u64 = 256 * 1024;
const MAX_OPEN_FILES: usize = 24;

enum Msg {
    Line { server: String, buffer: String, text: String },
    Flush(Sender<()>),
}

pub struct Logger {
    tx: Sender<Msg>,
}

impl Logger {
    pub fn new(root: PathBuf) -> Logger {
        let (tx, rx) = mpsc::channel::<Msg>();
        std::thread::Builder::new()
            .name("kirc-logs".into())
            .spawn(move || {
                let mut files: HashMap<PathBuf, File> = HashMap::new();
                for msg in rx {
                    match msg {
                        Msg::Flush(done) => {
                            let _ = done.send(());
                        }
                        Msg::Line { server, buffer, text } => {
                            let path = file_path(&root, &server, &buffer);
                            if !files.contains_key(&path) {
                                if files.len() >= MAX_OPEN_FILES {
                                    files.clear();
                                }
                                if let Some(dir) = path.parent() {
                                    let _ = fs::create_dir_all(dir);
                                }
                                match OpenOptions::new().create(true).append(true).open(&path) {
                                    Ok(f) => {
                                        files.insert(path.clone(), f);
                                    }
                                    Err(e) => {
                                        log::warn!("journal {} : {e}", path.display());
                                        continue;
                                    }
                                }
                            }
                            if let Some(f) = files.get_mut(&path) {
                                if let Err(e) = writeln!(f, "{text}") {
                                    log::warn!("journal {} : {e}", path.display());
                                    files.remove(&path);
                                }
                            }
                        }
                    }
                }
            })
            .expect("thread des journaux");
        Logger { tx }
    }

    /// Queues one line of a buffer (the console and non-conversation kinds are skipped).
    pub fn log(&self, server: &str, line: &Line) {
        if line.buffer.is_empty() {
            return;
        }
        if let Some(text) = format_line(line) {
            let _ = self.tx.send(Msg::Line { server: server.to_string(), buffer: line.buffer.clone(), text });
        }
    }

    /// Waits until everything queued so far is written.
    pub fn flush(&self) {
        let (done, wait) = mpsc::channel();
        if self.tx.send(Msg::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(Duration::from_secs(5));
        }
    }
}

/// A name that is safe as a Windows file or folder name, whatever the network sent.
pub fn sanitize(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    s = s.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if s.chars().count() > 80 {
        s = s.chars().take(80).collect();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    if s.is_empty() || reserved {
        s.insert(0, '_');
    }
    s
}

pub fn file_path(root: &Path, server: &str, buffer: &str) -> PathBuf {
    root.join(sanitize(server)).join(format!("{}.log", sanitize(&fold(buffer))))
}

fn clock(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map_or_else(|| "1970-01-01 00:00:00".into(), |t| t.format(TIME_FORMAT).to_string())
}

/// The text of a line in the log, or None for what is not worth keeping.
pub fn format_line(line: &Line) -> Option<String> {
    let body = text::strip_formatting(&line.text);
    let t = clock(line.time);
    Some(match line.kind {
        LineKind::Msg => format!("{t} <{}> {body}", line.nick),
        LineKind::Action => format!("{t} * {} {body}", line.nick),
        LineKind::Notice => format!("{t} -{}- {body}", line.nick),
        LineKind::Join | LineKind::Part | LineKind::Quit | LineKind::Kick | LineKind::Nick | LineKind::Mode | LineKind::Topic => {
            format!("{t} *** {body}")
        }
        LineKind::Info | LineKind::Error | LineKind::Motd | LineKind::Raw => return None,
    })
}

/// Reads a log line back; `buffer` is the window it will be shown in.
pub fn parse_line(buffer: &str, s: &str) -> Option<Line> {
    let (stamp, rest) = (s.get(..19)?, s.get(20..)?);
    let naive = NaiveDateTime::parse_from_str(stamp, TIME_FORMAT).ok()?;
    let time = Local.from_local_datetime(&naive).earliest()?.timestamp_millis();
    let mk = |kind, nick: &str, text: &str| Line { buffer: buffer.into(), kind, nick: nick.into(), text: text.into(), time, highlight: false, own: false };
    if let Some(r) = rest.strip_prefix('<') {
        let (nick, text) = r.split_once("> ")?;
        return Some(mk(LineKind::Msg, nick, text));
    }
    if let Some(r) = rest.strip_prefix("* ") {
        let (nick, text) = r.split_once(' ').unwrap_or((r, ""));
        return Some(mk(LineKind::Action, nick, text));
    }
    if let Some(r) = rest.strip_prefix('-') {
        let (nick, text) = r.split_once("- ")?;
        return Some(mk(LineKind::Notice, nick, text));
    }
    rest.strip_prefix("*** ").map(|text| mk(LineKind::Info, "", text))
}

/// The last `max` lines of a conversation older than `before_ms` (the window already holds the others).
pub fn tail(root: &Path, server: &str, buffer: &str, before_ms: i64, max: usize) -> Vec<Line> {
    let path = file_path(root, server, buffer);
    let Ok(mut f) = File::open(&path) else { return Vec::new() };
    let Ok(len) = f.metadata().map(|m| m.len()) else { return Vec::new() };
    let start = len.saturating_sub(TAIL_BYTES);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut raw = Vec::new();
    if f.take(TAIL_BYTES).read_to_end(&mut raw).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&raw);
    let mut lines = text.lines();
    if start > 0 {
        lines.next(); // a cut line
    }
    let mut out: Vec<Line> = lines.filter_map(|l| parse_line(buffer, l)).filter(|l| l.time < before_ms).collect();
    if out.len() > max {
        out.drain(..out.len() - max);
    }
    out
}

/// Deletes the log files not written to for `days` days (0 keeps everything).
pub fn sweep(root: &Path, days: u32) -> usize {
    if days == 0 {
        return 0;
    }
    let limit = SystemTime::now() - Duration::from_secs(u64::from(days) * 86_400);
    let mut removed = 0;
    let Ok(servers) = fs::read_dir(root) else { return 0 };
    for dir in servers.flatten().filter(|d| d.path().is_dir()) {
        let Ok(files) = fs::read_dir(dir.path()) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().is_some_and(|e| e == "log") && f.metadata().and_then(|m| m.modified()).is_ok_and(|m| m < limit) && fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: LineKind, nick: &str, text: &str, time: i64) -> Line {
        Line { buffer: "#a".into(), kind, nick: nick.into(), text: text.into(), time, highlight: false, own: false }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kirc-logs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn names_are_safe_for_windows() {
        assert_eq!(sanitize("#rust"), "#rust");
        let bad = sanitize("../../Windows/System32");
        assert!(!bad.contains('/') && !bad.contains(char::from(92)) && !bad.starts_with('.'), "{bad}");
        assert_eq!(sanitize("a:b*c?d"), "a_b_c_d");
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("nul.txt"), "_nul.txt");
        assert_eq!(sanitize("com1"), "_com1");
        assert_eq!(sanitize("com10"), "com10");
        assert_eq!(sanitize(".."), "_");
        assert_eq!(sanitize(""), "_");
        assert_eq!(sanitize("fin."), "fin");
        assert_eq!(sanitize(&"x".repeat(200)).len(), 80);
        // The file of a channel does not depend on the case it was written with.
        let root = Path::new("logs");
        assert_eq!(file_path(root, "Libera", "#Rust"), file_path(root, "Libera", "#rust"));
    }

    #[test]
    fn lines_survive_a_round_trip() {
        let t = Local.with_ymd_and_hms(2026, 10, 28, 21, 41, 3).unwrap().timestamp_millis();
        for (kind, nick, text, expect_kind) in [
            (LineKind::Msg, "alice", "bonjour <tout> le monde", LineKind::Msg),
            (LineKind::Action, "carol", "danse", LineKind::Action),
            (LineKind::Notice, "ChanServ", "bienvenue", LineKind::Notice),
            (LineKind::Join, "", "bob a rejoint #a", LineKind::Info),
        ] {
            let l = line(kind, nick, text, t);
            let s = format_line(&l).unwrap();
            assert!(s.starts_with("2026-10-28 21:41:03 "), "{s}");
            let back = parse_line("#a", &s).unwrap();
            assert_eq!((back.kind, back.nick.as_str(), back.text.as_str(), back.time), (expect_kind, nick, text, t), "{s}");
        }
        assert!(format_line(&line(LineKind::Info, "", "x", t)).is_none());
        assert!(format_line(&line(LineKind::Error, "", "x", t)).is_none());
        assert!(parse_line("#a", "pas une ligne").is_none());
        assert!(parse_line("#a", "2026-13-45 99:99:99 <x> y").is_none());
        // Formatting codes never reach the file.
        let s = format_line(&line(LineKind::Msg, "a", "\u{2}gras\u{3}04 rouge", t)).unwrap();
        assert!(s.ends_with("<a> gras rouge") && !s.contains('\u{2}'), "{s}");
    }

    #[test]
    fn writing_and_reading_back() {
        let root = tmp("rw");
        let logger = Logger::new(root.clone());
        let base = Local.with_ymd_and_hms(2026, 10, 28, 12, 0, 0).unwrap().timestamp_millis();
        for i in 0..10 {
            logger.log("Libera", &line(LineKind::Msg, "a", &format!("message {i}"), base + i * 1000));
        }
        let mut console = line(LineKind::Msg, "a", "jamais", base);
        console.buffer = String::new();
        logger.log("Libera", &console);
        logger.flush();
        assert!(!root.join("Libera").join("_.log").exists());
        let all = tail(&root, "Libera", "#A", i64::MAX, 100);
        assert_eq!(all.len(), 10);
        assert_eq!(all[9].text, "message 9");
        let before = tail(&root, "Libera", "#a", base + 5000, 100);
        assert_eq!(before.len(), 5, "only what is older than the window's first line");
        let last3 = tail(&root, "Libera", "#a", i64::MAX, 3);
        assert_eq!(last3.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["message 7", "message 8", "message 9"]);
        assert!(tail(&root, "Libera", "#autre", i64::MAX, 3).is_empty());
        assert!(tail(&root, "Autre", "#a", i64::MAX, 3).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_big_file_is_read_from_its_end() {
        let root = tmp("big");
        let path = file_path(&root, "S", "#a");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = File::create(&path).unwrap();
        let base = Local.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap().timestamp_millis();
        for i in 0..20_000 {
            writeln!(f, "{}", format_line(&line(LineKind::Msg, "n", &format!("ligne numéro {i} avec un peu de remplissage"), base + i * 1000)).unwrap()).unwrap();
        }
        drop(f);
        assert!(fs::metadata(&path).unwrap().len() > TAIL_BYTES * 2);
        let t = tail(&root, "S", "#a", i64::MAX, 50);
        assert_eq!(t.len(), 50);
        assert_eq!(t[49].text, "ligne numéro 19999 avec un peu de remplissage");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn old_files_are_swept() {
        let root = tmp("sweep");
        let dir = root.join("S");
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("#old.log");
        let new = dir.join("#new.log");
        fs::write(&old, "x").unwrap();
        fs::write(&new, "x").unwrap();
        let f = OpenOptions::new().write(true).open(&old).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(40 * 86_400)).unwrap();
        drop(f);
        assert_eq!(sweep(&root, 0), 0, "0 keeps everything");
        assert_eq!(sweep(&root, 30), 1);
        assert!(!old.exists() && new.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
