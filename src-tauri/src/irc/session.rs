//! The IRC protocol as a pure state machine: lines in, lines and UI events out.
//! No sockets, no clock, no threads -- `now` is passed in -- so everything here
//! is unit-tested without a server (see the tests at the bottom, and
//! tests/fake_server.rs for the network layer).

use super::event::{ChannelEntry, ConnState, Event, Line, LineKind, Out, UserEntry, WhoisInfo};
use super::message::{split_text, Message};
use base64::Engine as _;
use std::collections::{HashMap, HashSet};

/// Largest text sent in one PRIVMSG: the 512-byte limit minus the prefix the
/// server adds when relaying (nick!user@host PRIVMSG target :) with room to spare.
const MAX_TEXT: usize = 380;

/// Private conversations remembered per connection.
const MAX_QUERIES: usize = 300;

/// Rows kept of a channel list (the biggest networks have far more).
const MAX_LIST: usize = 30_000;

const SERVICES: [&str; 6] = ["nickserv", "chanserv", "memoserv", "operserv", "hostserv", "botserv"];

#[derive(Clone, Debug, Default)]
pub struct SessionConfig {
    pub nick: String,
    pub alt_nicks: Vec<String>,
    pub user: String,
    pub realname: String,
    /// Server password (PASS).
    pub password: Option<String>,
    /// SASL PLAIN account and password.
    pub sasl: Option<(String, String)>,
    /// Fallback when the server has no SASL: "PRIVMSG NickServ :IDENTIFY ...".
    pub nickserv_password: Option<String>,
    /// "#channel" or "#channel key".
    pub autojoin: Vec<String>,
    pub quit_message: String,
    pub part_message: String,
    pub version: String,
    /// Nick or mask patterns (`*` and `?`) whose messages are dropped.
    pub ignores: Vec<String>,
    /// Words that count as a mention, besides our nick.
    pub alert_words: Vec<String>,
}

struct Channel {
    name: String,
    users: HashMap<String, UserEntry>,
    /// Members received in 353 replies, applied at 366.
    pending: Option<Vec<UserEntry>>,
}

pub struct Session {
    cfg: SessionConfig,
    nick: String,
    network: String,
    registered: bool,
    /// (mode letter, symbol), highest rank first.
    prefixes: Vec<(char, char)>,
    chantypes: String,
    /// CHANMODES=A,B,C,D
    chanmodes: [String; 4],
    channels: HashMap<String, Channel>,
    queries: HashMap<String, String>,
    // Capability negotiation.
    cap_available: HashMap<String, String>,
    /// CAP REQ sent and not yet answered.
    cap_pending: usize,
    cap_enabled: HashSet<String>,
    cap_ended: bool,
    sasl_pending: bool,
    sasl_ok: bool,
    nick_tries: usize,
    // After the welcome.
    join_at: Option<i64>,
    rejoin: Vec<String>,
    keys: HashMap<String, String>,
    last_ctcp_reply: i64,
    whois: Option<WhoisInfo>,
    list: Option<Vec<ChannelEntry>>,
    /// Where the answer to the last typed command should be shown.
    reply_buffer: String,
    away: bool,
    quitting: bool,
}

/// RFC 1459 case folding: `[]\~` are the capitals of `{}|^`.
pub fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' => c.to_ascii_lowercase(),
            '[' => '{',
            ']' => '}',
            '\\' => '|',
            '~' => '^',
            _ => c,
        })
        .collect()
}

fn is_nick_char(c: char) -> bool {
    c.is_alphanumeric() || "[]\\`_^{|}-".contains(c)
}

/// Does `text` mention `nick` as a whole word, ignoring case?
pub fn mentions(text: &str, nick: &str) -> bool {
    if nick.is_empty() {
        return false;
    }
    let t = fold(text);
    let n = fold(nick);
    let mut from = 0;
    while let Some(i) = t[from..].find(&n) {
        let start = from + i;
        let end = start + n.len();
        let before = t[..start].chars().next_back();
        let after = t[end..].chars().next();
        if !before.is_some_and(is_nick_char) && !after.is_some_and(is_nick_char) {
            return true;
        }
        from = start + t[start..].chars().next().map_or(1, |c| c.len_utf8());
    }
    false
}

/// The full mask a user-typed ignore pattern stands for: "bob" is "bob!*@*", "*@host" is "*!*@host".
pub fn mask_of(pattern: &str) -> String {
    let p = fold(pattern.trim());
    if p.contains('!') {
        p
    } else if p.contains('@') {
        format!("*!{p}")
    } else {
        format!("{p}!*@*")
    }
}

/// Wildcard match: `*` is any run of characters, `?` exactly one. Both sides are already folded.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Milliseconds since the epoch for an IRCv3 `time` tag ("2026-10-28T12:00:00.000Z").
pub fn parse_server_time(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let millis = if b.get(19) == Some(&b'.') {
        let frac: String = s[20..].chars().take_while(|c| c.is_ascii_digit()).take(3).collect();
        format!("{frac:0<3}").parse::<i64>().ok()?
    } else {
        0
    };
    // Days since 1970-01-01 (Howard Hinnant's civil-from-days, inverted).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(((days * 24 + h) * 60 + mi) * 60_000 + se * 1000 + millis)
}

impl Session {
    pub fn new(cfg: SessionConfig) -> Self {
        let nick = cfg.nick.clone();
        let mut s = Session {
            cfg,
            nick,
            network: String::new(),
            registered: false,
            prefixes: vec![('o', '@'), ('v', '+')],
            chantypes: "#&".into(),
            chanmodes: Default::default(),
            channels: HashMap::new(),
            queries: HashMap::new(),
            cap_available: HashMap::new(),
            cap_pending: 0,
            cap_enabled: HashSet::new(),
            cap_ended: false,
            sasl_pending: false,
            sasl_ok: false,
            nick_tries: 0,
            join_at: None,
            rejoin: Vec::new(),
            keys: HashMap::new(),
            last_ctcp_reply: 0,
            whois: None,
            list: None,
            reply_buffer: String::new(),
            away: false,
            quitting: false,
        };
        s.chanmodes = ["beI".into(), "k".into(), "l".into(), "imnpstr".into()];
        s
    }

    pub fn nick(&self) -> &str {
        &self.nick
    }

    pub fn is_registered(&self) -> bool {
        self.registered
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting
    }

    /// Names of the channels we are on (for the "joined" list of the address book).
    pub fn channel_names(&self) -> Vec<String> {
        self.channels.values().map(|c| c.name.clone()).collect()
    }

    // ---------------------------------------------------------------- start

    /// The first lines of a connection: capability negotiation, then registration.
    pub fn start(&mut self) -> Vec<Out> {
        self.registered = false;
        self.cap_available.clear();
        self.cap_pending = 0;
        self.cap_enabled.clear();
        self.cap_ended = false;
        self.sasl_pending = false;
        self.sasl_ok = false;
        self.nick_tries = 0;
        self.whois = None;
        self.list = None;
        self.nick = self.cfg.nick.clone();
        self.prefixes = vec![('o', '@'), ('v', '+')];
        let mut out = vec![Out::SendNow("CAP LS 302".into())];
        if let Some(p) = &self.cfg.password {
            if !p.is_empty() {
                out.push(Out::SendNow(Message::new("PASS", &[p]).to_string()));
            }
        }
        out.push(Out::SendNow(Message::new("NICK", &[&self.nick]).to_string()));
        let user = if self.cfg.user.is_empty() { &self.nick } else { &self.cfg.user };
        let user: String = user.chars().filter(|c| !c.is_whitespace() && *c != '@').collect();
        let real = if self.cfg.realname.is_empty() { &self.nick } else { &self.cfg.realname };
        out.push(Out::SendNow(Message::new("USER", &[&user, "0", "*", real]).to_string()));
        out
    }

    /// The connection is gone: channels are forgotten (their windows stay open
    /// and are rejoined after the next connection).
    pub fn on_disconnect(&mut self, reason: &str, now: i64) -> Vec<Out> {
        let mut out = Vec::new();
        let was_registered = self.registered;
        self.registered = false;
        self.join_at = None;
        self.away = false;
        let mut names: Vec<String> = self.channels.values().map(|c| c.name.clone()).collect();
        names.sort();
        for name in names {
            self.rejoin_push(&name);
            out.push(Out::Ui(Event::Users { buffer: name.clone(), users: Vec::new() }));
            out.push(self.line(&name, LineKind::Error, "", format!("Déconnecté ({reason})"), now, false, false));
        }
        self.channels.clear();
        if was_registered {
            out.push(Out::Ui(Event::Away { away: false }));
        }
        out
    }

    fn rejoin_push(&mut self, name: &str) {
        if !self.rejoin.iter().any(|r| fold(r) == fold(name)) {
            self.rejoin.push(name.to_string());
        }
    }

    /// New ignore and alert lists (the settings changed while connected).
    pub fn set_filters(&mut self, ignores: Vec<String>, alert_words: Vec<String>) {
        self.cfg.ignores = ignores;
        self.cfg.alert_words = alert_words;
    }

    /// Is this message from someone on the ignore list?
    fn is_ignored(&self, msg: &Message) -> bool {
        if self.cfg.ignores.is_empty() {
            return false;
        }
        let Some(src) = msg.source() else { return false };
        let full = fold(&format!("{}!{}@{}", src.nick, src.user, src.host));
        self.cfg.ignores.iter().any(|p| glob_match(&mask_of(p), &full))
    }

    /// Does the text mention us, or one of the alert words?
    fn mentions_me(&self, text: &str) -> bool {
        mentions(text, &self.nick) || self.cfg.alert_words.iter().any(|w| mentions(text, w))
    }

    /// A channel left while the connection was down: it is not rejoined with the next one.
    pub fn forget_channel(&mut self, name: &str) {
        let key = fold(name);
        self.rejoin.retain(|r| fold(r.split(' ').next().unwrap_or("")) != key);
    }

    // ------------------------------------------------------------- helpers

    #[allow(clippy::too_many_arguments)]
    fn line(&self, buffer: &str, kind: LineKind, nick: &str, text: String, time: i64, highlight: bool, own: bool) -> Out {
        Out::Ui(Event::Line {
            line: Line { buffer: buffer.to_string(), kind, nick: nick.to_string(), text, time, highlight, own },
        })
    }

    fn info(&self, buffer: &str, text: impl Into<String>, now: i64) -> Out {
        self.line(buffer, LineKind::Info, "", text.into(), now, false, false)
    }

    fn error(&self, buffer: &str, text: impl Into<String>, now: i64) -> Out {
        self.line(buffer, LineKind::Error, "", text.into(), now, false, false)
    }

    fn is_channel(&self, name: &str) -> bool {
        name.chars().next().is_some_and(|c| self.chantypes.contains(c))
    }

    fn is_me(&self, nick: &str) -> bool {
        fold(nick) == fold(&self.nick)
    }

    /// The display name of a buffer we already know, or the name as given.
    fn channel_name(&self, name: &str) -> String {
        self.channels.get(&fold(name)).map_or_else(|| name.to_string(), |c| c.name.clone())
    }

    fn query_name(&mut self, nick: &str) -> String {
        // A flood of senders must not grow this table without bound.
        if self.queries.len() >= MAX_QUERIES && !self.queries.contains_key(&fold(nick)) {
            self.queries.clear();
        }
        self.queries.entry(fold(nick)).or_insert_with(|| nick.to_string()).clone()
    }

    fn user_entry(&self, nick_with_prefix: &str) -> UserEntry {
        let mut modes = String::new();
        let mut nick = nick_with_prefix;
        while let Some(c) = nick.chars().next() {
            if self.prefixes.iter().any(|&(_, s)| s == c) {
                modes.push(c);
                nick = &nick[c.len_utf8()..];
            } else {
                break;
            }
        }
        // Keep the highest rank first whatever order the server used.
        let rank = |c: char| self.prefixes.iter().position(|&(_, s)| s == c).unwrap_or(usize::MAX);
        let mut v: Vec<char> = modes.chars().collect();
        v.sort_by_key(|&c| rank(c));
        v.dedup();
        // "nick!user@host" when userhost-in-names is on: keep the nick.
        let nick = nick.split('!').next().unwrap_or(nick);
        UserEntry { nick: nick.to_string(), modes: v.into_iter().collect(), away: false }
    }

    fn sorted_users(ch: &Channel) -> Vec<UserEntry> {
        let mut v: Vec<UserEntry> = ch.users.values().cloned().collect();
        v.sort_by_key(|a| fold(&a.nick));
        v
    }

    fn time_of(msg: &Message, now: i64) -> i64 {
        msg.tag("time").and_then(parse_server_time).unwrap_or(now)
    }

    // ------------------------------------------------------ incoming lines

    pub fn handle(&mut self, msg: &Message, now: i64) -> Vec<Out> {
        let mut out = Vec::new();
        let time = Self::time_of(msg, now);
        let cmd = msg.command.as_str();
        match cmd {
            "PING" => out.push(Out::SendNow(Message::new("PONG", &[msg.last()]).to_string())),
            "PONG" => {}
            "CAP" => self.on_cap(msg, &mut out),
            "AUTHENTICATE" => self.on_authenticate(msg, &mut out),
            "NICK" => self.on_nick(msg, time, &mut out),
            "JOIN" => self.on_join(msg, time, &mut out),
            "PART" => self.on_part(msg, time, &mut out),
            "KICK" => self.on_kick(msg, time, &mut out),
            "QUIT" => self.on_quit(msg, time, &mut out),
            "MODE" => self.on_mode(msg, time, &mut out),
            "TOPIC" => self.on_topic(msg, time, &mut out),
            "PRIVMSG" => self.on_privmsg(msg, time, false, &mut out),
            "NOTICE" => self.on_privmsg(msg, time, true, &mut out),
            "INVITE" => {
                let who = msg.source().map_or("?", |s| s.nick);
                let text = format!("{who} vous invite sur {} (/join {})", msg.param(1), msg.param(1));
                out.push(self.line("", LineKind::Info, "", text, time, true, false));
            }
            "AWAY" => self.on_away(msg, &mut out),
            "ERROR" => out.push(self.error("", format!("Erreur du serveur : {}", msg.last()), now)),
            _ => {
                if let Ok(num) = cmd.parse::<u16>() {
                    self.on_numeric(num, msg, time, &mut out);
                }
            }
        }
        out
    }

    fn on_cap(&mut self, msg: &Message, out: &mut Vec<Out>) {
        let sub = msg.param(1).to_ascii_uppercase();
        match sub.as_str() {
            "LS" => {
                let more = msg.param(2) == "*";
                for cap in msg.last().split_whitespace() {
                    let (name, value) = cap.split_once('=').unwrap_or((cap, ""));
                    self.cap_available.insert(name.to_string(), value.to_string());
                }
                if more {
                    return;
                }
                let mut want: Vec<&str> = ["server-time", "multi-prefix", "away-notify"]
                    .into_iter()
                    .filter(|c| self.cap_available.contains_key(*c))
                    .collect();
                if self.cfg.sasl.is_some() {
                    // "sasl=PLAIN,EXTERNAL": the list, when given, must include PLAIN.
                    let plain = self.cap_available.get("sasl").map(|v| v.is_empty() || v.split(',').any(|m| m.eq_ignore_ascii_case("PLAIN")));
                    match plain {
                        Some(true) => want.push("sasl"),
                        _ => out.push(self.error("", "Ce serveur ne propose pas SASL PLAIN : authentification impossible", 0)),
                    }
                }
                // One request per capability: a refusal of one does not cancel the others.
                self.cap_pending = want.len();
                if want.is_empty() {
                    self.cap_end(out);
                }
                for c in want {
                    out.push(Out::SendNow(format!("CAP REQ :{c}")));
                }
            }
            "ACK" | "NAK" => {
                let acked = sub == "ACK";
                for cap in msg.last().split_whitespace() {
                    let name = cap.trim_start_matches(['-', '~', '=']);
                    if acked {
                        self.cap_enabled.insert(name.to_string());
                    } else if name == "sasl" && self.cfg.sasl.is_some() {
                        out.push(self.error("", "Le serveur a refusé SASL : authentification impossible", 0));
                    }
                    self.cap_pending = self.cap_pending.saturating_sub(1);
                }
                if self.cap_pending == 0 {
                    if self.cap_enabled.contains("sasl") && self.cfg.sasl.is_some() && !self.sasl_ok {
                        self.sasl_pending = true;
                        out.push(Out::SendNow("AUTHENTICATE PLAIN".into()));
                    } else {
                        self.cap_end(out);
                    }
                }
            }
            _ => {}
        }
    }

    fn cap_end(&mut self, out: &mut Vec<Out>) {
        if !self.cap_ended {
            self.cap_ended = true;
            out.push(Out::SendNow("CAP END".into()));
        }
    }

    fn on_authenticate(&mut self, msg: &Message, out: &mut Vec<Out>) {
        if !self.sasl_pending || msg.param(0) != "+" {
            return;
        }
        let Some((account, password)) = &self.cfg.sasl else { return };
        let raw = format!("{account}\0{account}\0{password}");
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
        let bytes = b64.as_bytes();
        for chunk in bytes.chunks(400) {
            out.push(Out::SendNow(format!("AUTHENTICATE {}", String::from_utf8_lossy(chunk))));
        }
        if bytes.len() % 400 == 0 {
            out.push(Out::SendNow("AUTHENTICATE +".into()));
        }
        // The password only lives as long as the exchange.
        self.sasl_pending = false;
    }

    fn on_nick(&mut self, msg: &Message, time: i64, out: &mut Vec<Out>) {
        let Some(src) = msg.source() else { return };
        let old = src.nick.to_string();
        let new = msg.param(0).to_string();
        let own = self.is_me(&old);
        if own {
            self.nick = new.clone();
            out.push(Out::Ui(Event::Me { nick: new.clone(), network: self.network.clone() }));
        }
        let mut names: Vec<String> = self.channels.keys().cloned().collect();
        names.sort();
        for key in names {
            let ch = self.channels.get_mut(&key).unwrap();
            if let Some(mut u) = ch.users.remove(&fold(&old)) {
                out.push(Out::Ui(Event::UserDel { buffer: ch.name.clone(), nick: old.clone() }));
                u.nick = new.clone();
                ch.users.insert(fold(&new), u.clone());
                out.push(Out::Ui(Event::UserSet { buffer: ch.name.clone(), user: u }));
                let text = if own {
                    format!("Vous êtes maintenant connu comme {new}")
                } else {
                    format!("{old} est maintenant connu comme {new}")
                };
                let name = ch.name.clone();
                out.push(self.line(&name, LineKind::Nick, &old, text, time, false, false));
            }
        }
        if let Some(q) = self.queries.remove(&fold(&old)) {
            let _ = q;
            self.queries.insert(fold(&new), new.clone());
            out.push(Out::Ui(Event::Query { buffer: new.clone() }));
            out.push(self.line(&new, LineKind::Nick, &old, format!("{old} est maintenant connu comme {new}"), time, false, false));
        }
    }

    fn on_join(&mut self, msg: &Message, time: i64, out: &mut Vec<Out>) {
        let Some(src) = msg.source() else { return };
        let chan = msg.param(0).to_string();
        if chan.is_empty() {
            return;
        }
        let key = fold(&chan);
        if self.is_me(src.nick) {
            self.channels.insert(
                key.clone(),
                Channel { name: chan.clone(), users: HashMap::new(), pending: None },
            );
            self.rejoin.retain(|r| fold(r.split(' ').next().unwrap_or("")) != key);
            out.push(Out::Ui(Event::Joined { buffer: chan.clone() }));
            out.push(self.line(&chan, LineKind::Join, src.nick, format!("Vous avez rejoint {chan}"), time, false, true));
            return;
        }
        let Some(ch) = self.channels.get_mut(&key) else { return };
        let entry = UserEntry { nick: src.nick.to_string(), modes: String::new(), away: false };
        ch.users.insert(fold(src.nick), entry.clone());
        let name = ch.name.clone();
        let host = if src.host.is_empty() { String::new() } else { format!(" ({}@{})", src.user, src.host) };
        out.push(Out::Ui(Event::UserSet { buffer: name.clone(), user: entry }));
        out.push(self.line(&name, LineKind::Join, src.nick, format!("{}{host} a rejoint {name}", src.nick), time, false, false));
    }

    fn on_part(&mut self, msg: &Message, time: i64, out: &mut Vec<Out>) {
        let Some(src) = msg.source() else { return };
        let key = fold(msg.param(0));
        let reason = msg.param(1);
        let why = if reason.is_empty() { String::new() } else { format!(" ({reason})") };
        if self.is_me(src.nick) {
            if let Some(ch) = self.channels.remove(&key) {
                out.push(self.line(&ch.name, LineKind::Part, src.nick, format!("Vous avez quitté {}{why}", ch.name), time, false, true));
                out.push(Out::Ui(Event::Parted { buffer: ch.name }));
            }
            return;
        }
        let Some(ch) = self.channels.get_mut(&key) else { return };
        if ch.users.remove(&fold(src.nick)).is_some() {
            let name = ch.name.clone();
            out.push(Out::Ui(Event::UserDel { buffer: name.clone(), nick: src.nick.to_string() }));
            out.push(self.line(&name, LineKind::Part, src.nick, format!("{} a quitté {name}{why}", src.nick), time, false, false));
        }
    }

    fn on_kick(&mut self, msg: &Message, time: i64, out: &mut Vec<Out>) {
        let kicker = msg.source().map_or("?", |s| s.nick).to_string();
        let key = fold(msg.param(0));
        let victim = msg.param(1).to_string();
        let reason = msg.param(2);
        let why = if reason.is_empty() { String::new() } else { format!(" ({reason})") };
        if self.is_me(&victim) {
            if let Some(ch) = self.channels.remove(&key) {
                let text = format!("{kicker} vous a expulsé de {}{why}", ch.name);
                out.push(self.line(&ch.name, LineKind::Kick, &kicker, text, time, true, false));
                out.push(Out::Ui(Event::Parted { buffer: ch.name }));
            }
            return;
        }
        let Some(ch) = self.channels.get_mut(&key) else { return };
        if ch.users.remove(&fold(&victim)).is_some() {
            let name = ch.name.clone();
            out.push(Out::Ui(Event::UserDel { buffer: name.clone(), nick: victim.clone() }));
            out.push(self.line(&name, LineKind::Kick, &kicker, format!("{kicker} a expulsé {victim}{why}"), time, false, false));
        }
    }

    fn on_quit(&mut self, msg: &Message, time: i64, out: &mut Vec<Out>) {
        let Some(src) = msg.source() else { return };
        let reason = msg.param(0);
        let why = if reason.is_empty() { String::new() } else { format!(" ({reason})") };
        let host = if src.host.is_empty() { String::new() } else { format!(" ({}@{})", src.user, src.host) };
        let mut keys: Vec<String> = self.channels.keys().cloned().collect();
        keys.sort();
        for key in keys {
            let ch = self.channels.get_mut(&key).unwrap();
            if ch.users.remove(&fold(src.nick)).is_some() {
                let name = ch.name.clone();
                out.push(Out::Ui(Event::UserDel { buffer: name.clone(), nick: src.nick.to_string() }));
                out.push(self.line(&name, LineKind::Quit, src.nick, format!("{}{host} a quitté IRC{why}", src.nick), time, false, false));
            }
        }
        if let Some(q) = self.queries.get(&fold(src.nick)).cloned() {
            out.push(self.line(&q, LineKind::Quit, src.nick, format!("{}{host} a quitté IRC{why}", src.nick), time, false, false));
        }
    }

    fn on_away(&mut self, msg: &Message, out: &mut Vec<Out>) {
        let Some(src) = msg.source() else { return };
        let away = !msg.params.is_empty();
        let mut keys: Vec<String> = self.channels.keys().cloned().collect();
        keys.sort();
        for key in keys {
            let ch = self.channels.get_mut(&key).unwrap();
            if let Some(u) = ch.users.get_mut(&fold(src.nick)) {
                u.away = away;
                out.push(Out::Ui(Event::UserSet { buffer: ch.name.clone(), user: u.clone() }));
            }
        }
    }

    fn on_topic(&mut self, msg: &Message, time: i64, out: &mut Vec<Out>) {
        let who = msg.source().map_or("?", |s| s.nick).to_string();
        let name = self.channel_name(msg.param(0));
        let topic = msg.param(1).to_string();
        out.push(Out::Ui(Event::Topic { buffer: name.clone(), topic: topic.clone() }));
        let text = if topic.is_empty() {
            format!("{who} a effacé le sujet")
        } else {
            format!("{who} a changé le sujet : {topic}")
        };
        out.push(self.line(&name, LineKind::Topic, &who, text, time, false, false));
    }

    /// Letters of the channel modes that take a parameter, per CHANMODES and PREFIX.
    fn on_mode(&mut self, msg: &Message, time: i64, out: &mut Vec<Out>) {
        let who = msg.source().map_or("?", |s| s.nick).to_string();
        let target = msg.param(0).to_string();
        let modes = msg.param(1).to_string();
        let args: Vec<&str> = msg.params.iter().skip(2).map(String::as_str).collect();
        if !self.is_channel(&target) {
            let text = format!("{who} définit le mode {} {}", modes, args.join(" "));
            out.push(self.line("", LineKind::Mode, &who, text.trim_end().to_string(), time, false, false));
            return;
        }
        let name = self.channel_name(&target);
        let key = fold(&name);
        let mut adding = true;
        let mut arg_i = 0;
        let mut changes: Vec<(String, char, bool)> = Vec::new(); // (nick, symbol, added)
        for c in modes.chars() {
            match c {
                '+' => adding = true,
                '-' => adding = false,
                _ => {
                    if let Some(&(_, sym)) = self.prefixes.iter().find(|&&(m, _)| m == c) {
                        if let Some(nick) = args.get(arg_i) {
                            changes.push((nick.to_string(), sym, adding));
                        }
                        arg_i += 1;
                    } else if self.chanmodes[0].contains(c)
                        || self.chanmodes[1].contains(c)
                        || (self.chanmodes[2].contains(c) && adding)
                    {
                        // List and key modes always take a parameter, limit-like ones only when set.
                        arg_i += 1;
                    }
                }
            }
        }
        if let Some(ch) = self.channels.get_mut(&key) {
            for (nick, sym, added) in changes {
                if let Some(u) = ch.users.get_mut(&fold(&nick)) {
                    u.modes.retain(|m| m != sym);
                    if added {
                        u.modes.push(sym);
                    }
                    let rank = |c: char| self.prefixes.iter().position(|&(_, s)| s == c).unwrap_or(usize::MAX);
                    let mut v: Vec<char> = u.modes.chars().collect();
                    v.sort_by_key(|&c| rank(c));
                    u.modes = v.into_iter().collect();
                    out.push(Out::Ui(Event::UserSet { buffer: ch.name.clone(), user: u.clone() }));
                }
            }
        }
        let text = format!("{who} définit le mode {} {}", modes, args.join(" "));
        out.push(self.line(&name, LineKind::Mode, &who, text.trim_end().to_string(), time, false, false));
    }

    fn on_privmsg(&mut self, msg: &Message, time: i64, notice: bool, out: &mut Vec<Out>) {
        if self.is_ignored(msg) {
            return;
        }
        let src_nick = msg.source().map_or("", |s| s.nick).to_string();
        let from_server = msg.source().map_or(true, |s| s.user.is_empty() && s.host.is_empty() && s.nick.contains('.'));
        let target = msg.param(0).to_string();
        let text = msg.param(1).to_string();

        // CTCP: \x01COMMAND args\x01
        if text.starts_with('\u{1}') {
            let body = text.trim_matches('\u{1}');
            let (cmd, rest) = body.split_once(' ').unwrap_or((body, ""));
            if notice {
                // A CTCP reply (VERSION, PING...): show it in the console.
                let t = format!("Réponse CTCP {} de {src_nick} : {rest}", cmd.to_ascii_uppercase());
                out.push(self.line("", LineKind::Info, "", t, time, false, false));
                return;
            }
            if cmd.eq_ignore_ascii_case("ACTION") {
                let Some(buffer) = self.message_buffer(&src_nick, &target, out) else { return };
                let hl = self.mentions_me(rest) || !self.is_channel(&target);
                out.push(self.line(&buffer, LineKind::Action, &src_nick, rest.to_string(), time, hl, false));
                return;
            }
            self.ctcp_reply(&src_nick, cmd, rest, time, out);
            return;
        }

        if notice {
            if from_server || src_nick.is_empty() {
                out.push(self.line("", LineKind::Notice, &src_nick, text, time, false, false));
                return;
            }
            // NickServ told us we are identified: no need to wait to join.
            if fold(&src_nick) == "nickserv" && self.join_at.is_some() {
                let t = fold(&text);
                if ["identified", "recognized", "recognised", "reconnu", "identifi", "accepted", "logged in"]
                    .iter()
                    .any(|k| t.contains(k))
                {
                    self.join_at = Some(0);
                }
            }
            let buffer = if self.is_channel(&target) {
                // A notice to a channel we are not on goes to the console, never to a new buffer.
                self.channels.get(&fold(&target)).map_or_else(String::new, |c| c.name.clone())
            } else if self.queries.contains_key(&fold(&src_nick)) {
                self.queries[&fold(&src_nick)].clone()
            } else {
                String::new()
            };
            let hl = self.is_channel(&target) && self.mentions_me(&text);
            out.push(self.line(&buffer, LineKind::Notice, &src_nick, text, time, hl, false));
            return;
        }

        let Some(buffer) = self.message_buffer(&src_nick, &target, out) else { return };
        let hl = !self.is_channel(&target) || self.mentions_me(&text);
        out.push(self.line(&buffer, LineKind::Msg, &src_nick, text, time, hl, false));
    }

    /// The buffer an incoming message belongs to; opens a query for a private message.
    /// `None` for what must be ignored: a message to a channel we are not on, or without a sender.
    fn message_buffer(&mut self, from: &str, target: &str, out: &mut Vec<Out>) -> Option<String> {
        if self.is_channel(target) {
            return self.channels.get(&fold(target)).map(|c| c.name.clone());
        }
        if from.is_empty() {
            return None;
        }
        let fresh = !self.queries.contains_key(&fold(from));
        let q = self.query_name(from);
        if fresh {
            out.push(Out::Ui(Event::Query { buffer: q.clone() }));
        }
        Some(q)
    }

    fn ctcp_reply(&mut self, from: &str, cmd: &str, args: &str, now: i64, out: &mut Vec<Out>) {
        let up = cmd.to_ascii_uppercase();
        let reply = match up.as_str() {
            "VERSION" => format!("VERSION {}", self.cfg.version),
            "PING" => format!("PING {args}"),
            "TIME" => "TIME (non communiquée)".to_string(),
            "SOURCE" => "SOURCE https://github.com/Crazyshouby/KIRC".to_string(),
            _ => return,
        };
        // At most one reply per second: a flood of CTCP requests is a classic annoyance.
        if now - self.last_ctcp_reply < 1000 && self.last_ctcp_reply != 0 {
            return;
        }
        self.last_ctcp_reply = now;
        out.push(Out::Send(Message::new("NOTICE", &[from, &format!("\u{1}{reply}\u{1}")]).to_string()));
        out.push(self.info("", format!("Requête CTCP {up} de {from}"), now));
    }

    // ------------------------------------------------------------ numerics

    fn on_numeric(&mut self, num: u16, msg: &Message, time: i64, out: &mut Vec<Out>) {
        let rest = || msg.params.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
        match num {
            1 => {
                self.registered = true;
                if !msg.param(0).is_empty() {
                    self.nick = msg.param(0).to_string();
                }
                out.push(Out::Ui(Event::Status { state: ConnState::Connected, text: msg.last().to_string() }));
                out.push(Out::Ui(Event::Me { nick: self.nick.clone(), network: self.network.clone() }));
                out.push(self.line("", LineKind::Info, "", msg.last().to_string(), time, false, false));
                self.after_welcome(time, out);
            }
            2 | 3 | 4 | 250..=255 | 265 | 266 => {
                out.push(self.line("", LineKind::Info, "", rest(), time, false, false));
            }
            5 => self.on_isupport(msg, out, time),
            375 | 372 | 377 | 378 => {
                out.push(self.line("", LineKind::Motd, "", msg.last().to_string(), time, false, false));
            }
            376 | 422 => {
                out.push(self.line("", LineKind::Motd, "", msg.last().to_string(), time, false, false));
            }
            331 => {
                let name = self.channel_name(msg.param(1));
                out.push(Out::Ui(Event::Topic { buffer: name.clone(), topic: String::new() }));
            }
            332 => {
                let name = self.channel_name(msg.param(1));
                let topic = msg.param(2).to_string();
                out.push(Out::Ui(Event::Topic { buffer: name.clone(), topic: topic.clone() }));
                out.push(self.line(&name, LineKind::Topic, "", format!("Sujet : {topic}"), time, false, false));
            }
            333 => {
                let name = self.channel_name(msg.param(1));
                out.push(self.line(&name, LineKind::Info, "", format!("Sujet défini par {}", msg.param(2)), time, false, false));
            }
            353 => {
                let key = fold(msg.param(2));
                let entries: Vec<UserEntry> = msg.last().split_whitespace().map(|n| self.user_entry(n)).collect();
                if let Some(ch) = self.channels.get_mut(&key) {
                    ch.pending.get_or_insert_with(Vec::new).extend(entries);
                }
            }
            366 => {
                let key = fold(msg.param(1));
                if let Some(ch) = self.channels.get_mut(&key) {
                    if let Some(list) = ch.pending.take() {
                        ch.users = list.into_iter().map(|u| (fold(&u.nick), u)).collect();
                        let users = Self::sorted_users(ch);
                        out.push(Out::Ui(Event::Users { buffer: ch.name.clone(), users }));
                    }
                }
            }
            324 => {
                let name = self.channel_name(msg.param(1));
                out.push(self.line(&name, LineKind::Info, "", format!("Modes de {name} : {}", msg.params.iter().skip(2).cloned().collect::<Vec<_>>().join(" ")), time, false, false));
            }
            329 => {}
            // WHOIS / WHOWAS
            // WHOIS is collected and shown as one card (see Event::Whois); WHOWAS stays a line.
            311 => {
                self.whois = Some(WhoisInfo {
                    nick: msg.param(1).to_string(),
                    user: msg.param(2).to_string(),
                    host: msg.param(3).to_string(),
                    realname: msg.last().to_string(),
                    ..Default::default()
                });
            }
            314 => {
                let t = format!("{} était {}@{} : {}", msg.param(1), msg.param(2), msg.param(3), msg.last());
                out.push(self.line(&self.reply_buffer.clone(), LineKind::Info, "", t, time, false, false));
            }
            312 => {
                if let Some(w) = self.whois.as_mut() {
                    w.server = msg.param(2).to_string();
                    w.server_info = msg.last().to_string();
                }
            }
            313 => {
                if let Some(w) = self.whois.as_mut() {
                    w.operator = true;
                }
            }
            317 => {
                if let Some(w) = self.whois.as_mut() {
                    w.idle = msg.param(2).parse().ok();
                    w.signon = msg.param(3).parse().ok();
                }
            }
            319 => {
                if let Some(w) = self.whois.as_mut() {
                    w.channels = msg.last().to_string();
                }
            }
            330 => {
                if let Some(w) = self.whois.as_mut() {
                    w.account = msg.param(2).to_string();
                }
            }
            671 => {
                if let Some(w) = self.whois.as_mut() {
                    w.secure = true;
                }
            }
            318 => {
                if let Some(w) = self.whois.take() {
                    out.push(Out::Ui(Event::Whois { info: Box::new(w) }));
                }
            }
            369 => {}
            301 => {
                if let Some(w) = self.whois.as_mut() {
                    w.away = msg.last().to_string();
                } else {
                    let t = format!("{} est absent : {}", msg.param(1), msg.last());
                    let b = self.reply_target_for(msg.param(1));
                    out.push(self.line(&b, LineKind::Info, "", t, time, false, false));
                }
            }
            // LIST: collected whole (a big network sends tens of thousands), shown in its own window.
            321 => {
                self.list = Some(Vec::new());
                out.push(Out::Ui(Event::ListStart));
            }
            322 => {
                if let Some(l) = self.list.as_mut() {
                    if l.len() < MAX_LIST {
                        l.push(ChannelEntry { name: msg.param(1).to_string(), users: msg.param(2).parse().unwrap_or(0), topic: msg.last().to_string() });
                    }
                }
            }
            323 => {
                if let Some(entries) = self.list.take() {
                    out.push(Out::Ui(Event::ChannelList { entries }));
                }
            }
            305 => {
                self.away = false;
                out.push(Out::Ui(Event::Away { away: false }));
                out.push(self.info(&self.reply_buffer.clone(), "Vous n'êtes plus marqué absent", time));
            }
            306 => {
                self.away = true;
                out.push(Out::Ui(Event::Away { away: true }));
                out.push(self.info(&self.reply_buffer.clone(), "Vous êtes marqué absent", time));
            }
            // SASL
            900 => out.push(self.info("", format!("Connecté en tant que {}", msg.param(2)), time)),
            903 => {
                self.sasl_ok = true;
                self.sasl_pending = false;
                out.push(self.info("", "Authentification SASL réussie", time));
                self.cap_end(out);
            }
            902 | 904 | 905 | 906 | 907 => {
                self.sasl_pending = false;
                out.push(self.error("", format!("Authentification SASL échouée : {}", msg.last()), time));
                self.cap_end(out);
            }
            // Nick in use / erroneous, while registering.
            432 | 433 | 436 if !self.registered => {
                self.nick_tries += 1;
                let next = match self.cfg.alt_nicks.get(self.nick_tries - 1) {
                    Some(n) if !n.is_empty() => n.clone(),
                    _ => format!("{}{}", self.cfg.nick, "_".repeat(self.nick_tries - self.cfg.alt_nicks.len().min(self.nick_tries - 1))),
                };
                out.push(self.info("", format!("Pseudo {} indisponible, essai de {next}", msg.param(1)), time));
                self.nick = next.clone();
                out.push(Out::SendNow(Message::new("NICK", &[&next]).to_string()));
            }
            464 => out.push(self.error("", "Mot de passe du serveur refusé", time)),
            // Everything else in the error ranges: show it where the user is looking.
            400..=599 => {
                let b = self.reply_buffer.clone();
                out.push(self.error(&b, rest(), time));
            }
            _ => {}
        }
    }

    fn reply_target_for(&self, nick: &str) -> String {
        self.queries.get(&fold(nick)).cloned().unwrap_or_else(|| self.reply_buffer.clone())
    }

    fn on_isupport(&mut self, msg: &Message, out: &mut Vec<Out>, now: i64) {
        let n = msg.params.len();
        for tok in msg.params.iter().take(n.saturating_sub(1)).skip(1) {
            let (key, val) = tok.split_once('=').unwrap_or((tok.as_str(), ""));
            match key {
                "PREFIX" => {
                    // (ov)@+
                    if let Some(rest) = val.strip_prefix('(') {
                        if let Some((modes, syms)) = rest.split_once(')') {
                            let v: Vec<(char, char)> = modes.chars().zip(syms.chars()).collect();
                            if !v.is_empty() {
                                self.prefixes = v;
                            }
                        }
                    }
                }
                "CHANTYPES" if !val.is_empty() => self.chantypes = val.to_string(),
                "CHANMODES" => {
                    for (i, part) in val.split(',').take(4).enumerate() {
                        self.chanmodes[i] = part.to_string();
                    }
                }
                "NETWORK" => {
                    self.network = val.to_string();
                    out.push(Out::Ui(Event::Me { nick: self.nick.clone(), network: self.network.clone() }));
                    out.push(self.info("", format!("Réseau : {val}"), now));
                }
                _ => {}
            }
        }
    }

    fn after_welcome(&mut self, now: i64, out: &mut Vec<Out>) {
        if !self.sasl_ok {
            if let Some(p) = self.cfg.nickserv_password.clone().filter(|p| !p.is_empty()) {
                out.push(Out::Send(Message::new("PRIVMSG", &["NickServ", &format!("IDENTIFY {p}")]).to_string()));
                out.push(self.info("", "Identification auprès de NickServ…", now));
                // Join once NickServ confirms, or after a short wait.
                self.join_at = Some(now + 3000);
                return;
            }
        }
        self.do_join(now, out);
    }

    fn do_join(&mut self, _now: i64, out: &mut Vec<Out>) {
        self.join_at = None;
        let mut list: Vec<String> = self.cfg.autojoin.clone();
        for r in &self.rejoin {
            if !list.iter().any(|a| fold(a.split(' ').next().unwrap_or("")) == fold(r.split(' ').next().unwrap_or(""))) {
                list.push(r.clone());
            }
        }
        self.rejoin.clear();
        let mut chans: Vec<String> = Vec::new();
        for entry in list {
            let mut it = entry.split_whitespace();
            let Some(c) = it.next() else { continue };
            chans.push(c.to_string());
            if let Some(k) = it.next() {
                self.keys.insert(fold(c), k.to_string());
            }
        }
        // Channels with a key first, as the protocol wants.
        chans.sort_by_key(|c| !self.keys.contains_key(&fold(c)));
        for c in chans {
            let line = match self.keys.get(&fold(&c)) {
                Some(k) => Message::new("JOIN", &[&c, k]).to_string(),
                None => Message::new("JOIN", &[&c]).to_string(),
            };
            out.push(Out::Send(line));
        }
    }

    /// Called about once a second by the network layer.
    pub fn tick(&mut self, now: i64) -> Vec<Out> {
        let mut out = Vec::new();
        if let Some(at) = self.join_at {
            if now >= at {
                self.do_join(now, &mut out);
            }
        }
        out
    }

    // ------------------------------------------------------- typed input

    /// A line typed in `buffer` ("" = the console).
    pub fn user_input(&mut self, buffer: &str, text: &str, now: i64) -> Vec<Out> {
        let mut out = Vec::new();
        let text = text.trim_end_matches(['\r', '\n']);
        if text.trim().is_empty() {
            return out;
        }
        self.reply_buffer = buffer.to_string();
        let Some(cmdline) = text.strip_prefix('/').filter(|r| !r.starts_with('/')) else {
            let plain = text.strip_prefix('/').map_or(text, |_| &text[1..]);
            if buffer.is_empty() {
                out.push(self.error("", "Aucun salon ici : tapez /join #salon ou /msg pseudo texte (/aide pour la liste)", now));
            } else {
                self.say(buffer, plain, false, now, &mut out);
            }
            return out;
        };
        let (cmd, rest) = cmdline.split_once(' ').unwrap_or((cmdline, ""));
        let rest = rest.trim();
        self.command(&cmd.to_ascii_lowercase(), rest, buffer, now, &mut out);
        out
    }

    /// Sends `text` to `target` (split into several lines if too long) and echoes it.
    fn say(&mut self, target: &str, text: &str, action: bool, now: i64, out: &mut Vec<Out>) {
        let is_chan = self.is_channel(target);
        let (shown, kind) = (if is_chan { self.channel_name(target) } else { target.to_string() }, if action { LineKind::Action } else { LineKind::Msg });
        let buffer = if is_chan {
            shown
        } else if SERVICES.contains(&fold(target).as_str()) {
            String::new()
        } else {
            let fresh = !self.queries.contains_key(&fold(target));
            let q = self.query_name(target);
            if fresh {
                out.push(Out::Ui(Event::Query { buffer: q.clone() }));
            }
            q
        };
        for part in split_text(text, MAX_TEXT) {
            let wire = if action { format!("\u{1}ACTION {part}\u{1}") } else { part.clone() };
            out.push(Out::Send(Message::new("PRIVMSG", &[target, &wire]).to_string()));
            let mut shown_text = part;
            if buffer.is_empty() {
                // Console echo for services: hide what follows IDENTIFY / REGISTER / GROUP.
                let low = shown_text.to_ascii_lowercase();
                for secret in ["identify ", "id ", "register ", "group ", "ghost ", "release ", "recover "] {
                    if low.starts_with(secret) {
                        let words: Vec<&str> = shown_text.split(' ').collect();
                        shown_text = format!("{} ***", words.first().copied().unwrap_or(""));
                        break;
                    }
                }
                shown_text = format!("→ {target} : {shown_text}");
            }
            out.push(self.line(&buffer, kind, &self.nick.clone(), shown_text, now, false, true));
        }
    }

    fn command(&mut self, cmd: &str, rest: &str, buffer: &str, now: i64, out: &mut Vec<Out>) {
        let in_chan = self.is_channel(buffer);
        let first = rest.split_whitespace().next().unwrap_or("");
        match cmd {
            "join" | "j" => {
                if rest.is_empty() {
                    out.push(self.error(buffer, "Usage : /join #salon [clé]", now));
                    return;
                }
                let mut it = rest.split_whitespace();
                let chans = it.next().unwrap_or("");
                let keys: Vec<&str> = it.next().map(|k| k.split(',').collect()).unwrap_or_default();
                let list: Vec<String> = chans
                    .split(',')
                    .filter(|c| !c.is_empty())
                    .map(|c| if self.is_channel(c) { c.to_string() } else { format!("#{c}") })
                    .collect();
                for (i, c) in list.iter().enumerate() {
                    match keys.get(i) {
                        Some(k) => {
                            self.keys.insert(fold(c), k.to_string());
                            out.push(Out::Send(Message::new("JOIN", &[c, k]).to_string()));
                        }
                        None => out.push(Out::Send(Message::new("JOIN", &[c]).to_string())),
                    }
                }
            }
            "part" | "leave" | "p" => {
                let (chan, reason) = if self.is_channel(first) {
                    (first.to_string(), rest[first.len()..].trim().to_string())
                } else if in_chan {
                    (buffer.to_string(), rest.to_string())
                } else {
                    out.push(self.error(buffer, "Usage : /part [#salon] [message]", now));
                    return;
                };
                let reason = if reason.is_empty() { self.cfg.part_message.clone() } else { reason };
                out.push(Out::Send(Message::new("PART", &[&chan, &reason]).to_string()));
            }
            "msg" | "m" | "privmsg" => {
                let Some((target, text)) = rest.split_once(' ') else {
                    out.push(self.error(buffer, "Usage : /msg pseudo texte", now));
                    return;
                };
                self.say(target, text.trim(), false, now, out);
            }
            "query" | "q" => {
                if first.is_empty() {
                    out.push(self.error(buffer, "Usage : /query pseudo [texte]", now));
                    return;
                }
                let q = self.query_name(first);
                out.push(Out::Ui(Event::Query { buffer: q }));
                let text = rest[first.len()..].trim();
                if !text.is_empty() {
                    self.say(first, text, false, now, out);
                }
            }
            "me" | "action" => {
                if buffer.is_empty() {
                    out.push(self.error(buffer, "Utilisez /me dans un salon ou une conversation", now));
                } else {
                    self.say(buffer, rest, true, now, out);
                }
            }
            "notice" => {
                let Some((target, text)) = rest.split_once(' ') else {
                    out.push(self.error(buffer, "Usage : /notice cible texte", now));
                    return;
                };
                out.push(Out::Send(Message::new("NOTICE", &[target, text.trim()]).to_string()));
                let b = if self.is_channel(target) { self.channel_name(target) } else { buffer.to_string() };
                out.push(self.line(&b, LineKind::Notice, &self.nick.clone(), format!("→ {target} : {}", text.trim()), now, false, true));
            }
            "nick" | "n" => {
                if first.is_empty() {
                    out.push(self.error(buffer, "Usage : /nick nouveau_pseudo", now));
                } else {
                    out.push(Out::SendNow(Message::new("NICK", &[first]).to_string()));
                }
            }
            "quit" | "exit" | "disconnect" | "deconnecter" => {
                let reason = if rest.is_empty() { self.cfg.quit_message.clone() } else { rest.to_string() };
                self.quitting = true;
                out.push(Out::SendNow(Message::new("QUIT", &[&reason]).to_string()));
                out.push(Out::Quit);
            }
            "topic" | "sujet" => {
                let (chan, text) = if self.is_channel(first) {
                    (first.to_string(), rest[first.len()..].trim().to_string())
                } else if in_chan {
                    (buffer.to_string(), rest.to_string())
                } else {
                    out.push(self.error(buffer, "Usage : /topic [#salon] [nouveau sujet]", now));
                    return;
                };
                if text.is_empty() {
                    out.push(Out::Send(Message::new("TOPIC", &[&chan]).to_string()));
                } else {
                    out.push(Out::Send(Message::new("TOPIC", &[&chan, &text]).to_string()));
                }
            }
            "kick" | "k" => {
                if !in_chan || first.is_empty() {
                    out.push(self.error(buffer, "Usage (dans un salon) : /kick pseudo [raison]", now));
                    return;
                }
                let reason = rest[first.len()..].trim();
                out.push(Out::Send(Message::new("KICK", &[buffer, first, reason]).to_string()));
            }
            "ban" | "unban" => {
                if !in_chan || first.is_empty() {
                    out.push(self.error(buffer, "Usage (dans un salon) : /ban pseudo|masque", now));
                    return;
                }
                let mask = if first.contains('!') || first.contains('@') || first.contains('*') { first.to_string() } else { format!("{first}!*@*") };
                let m = if cmd == "ban" { "+b" } else { "-b" };
                out.push(Out::Send(Message::new("MODE", &[buffer, m, &mask]).to_string()));
            }
            "mode" => {
                let mut args: Vec<String> = rest.split_whitespace().map(String::from).collect();
                if args.is_empty() {
                    args.push(if in_chan { buffer.to_string() } else { self.nick.clone() });
                } else if in_chan && (args[0].starts_with('+') || args[0].starts_with('-')) {
                    args.insert(0, buffer.to_string());
                }
                let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                out.push(Out::Send(Message::new("MODE", &refs).to_string()));
            }
            "op" | "deop" | "voice" | "devoice" | "halfop" | "dehalfop" => {
                if !in_chan || first.is_empty() {
                    out.push(self.error(buffer, format!("Usage (dans un salon) : /{cmd} pseudo [pseudo…]"), now));
                    return;
                }
                let (sign, letter) = match cmd {
                    "op" => ('+', 'o'),
                    "deop" => ('-', 'o'),
                    "voice" => ('+', 'v'),
                    "devoice" => ('-', 'v'),
                    "halfop" => ('+', 'h'),
                    _ => ('-', 'h'),
                };
                for group in rest.split_whitespace().collect::<Vec<_>>().chunks(4) {
                    let modes: String = std::iter::once(sign).chain(group.iter().map(|_| letter)).collect();
                    let mut args = vec![buffer, modes.as_str()];
                    args.extend(group.iter().copied());
                    out.push(Out::Send(Message::new("MODE", &args).to_string()));
                }
            }
            "whois" | "wi" => {
                if first.is_empty() {
                    out.push(self.error(buffer, "Usage : /whois pseudo", now));
                } else {
                    out.push(Out::Send(Message::new("WHOIS", &[first, first]).to_string()));
                }
            }
            "whowas" => {
                if first.is_empty() {
                    out.push(self.error(buffer, "Usage : /whowas pseudo", now));
                } else {
                    out.push(Out::Send(Message::new("WHOWAS", &[first]).to_string()));
                }
            }
            "away" | "absent" => {
                if rest.is_empty() {
                    out.push(Out::Send("AWAY".into()));
                } else {
                    out.push(Out::Send(Message::new("AWAY", &[rest]).to_string()));
                }
            }
            "back" => out.push(Out::Send("AWAY".into())),
            "invite" => {
                let mut it = rest.split_whitespace();
                let Some(nick) = it.next() else {
                    out.push(self.error(buffer, "Usage : /invite pseudo [#salon]", now));
                    return;
                };
                let chan = it.next().map(str::to_string).or_else(|| in_chan.then(|| buffer.to_string()));
                match chan {
                    Some(c) => out.push(Out::Send(Message::new("INVITE", &[nick, &c]).to_string())),
                    None => out.push(self.error(buffer, "Précisez le salon : /invite pseudo #salon", now)),
                }
            }
            "list" | "liste" => {
                if rest.is_empty() {
                    out.push(Out::Send("LIST".into()));
                } else {
                    out.push(Out::Send(Message::new("LIST", &[rest]).to_string()));
                }
            }
            "names" => {
                let chan = if self.is_channel(first) { first.to_string() } else { buffer.to_string() };
                if self.is_channel(&chan) {
                    out.push(Out::Send(Message::new("NAMES", &[&chan]).to_string()));
                }
            }
            "ctcp" => {
                let mut it = rest.splitn(3, ' ');
                let (Some(t), Some(c)) = (it.next(), it.next()) else {
                    out.push(self.error(buffer, "Usage : /ctcp pseudo VERSION|PING|TIME", now));
                    return;
                };
                let args = it.next().unwrap_or("");
                let body = if args.is_empty() { c.to_ascii_uppercase() } else { format!("{} {args}", c.to_ascii_uppercase()) };
                out.push(Out::Send(Message::new("PRIVMSG", &[t, &format!("\u{1}{body}\u{1}")]).to_string()));
                out.push(self.info(buffer, format!("Requête CTCP {} envoyée à {t}", c.to_ascii_uppercase()), now));
            }
            "ns" | "nickserv" | "cs" | "chanserv" | "ms" | "memoserv" | "os" | "operserv" | "hs" | "hostserv" | "bs" | "botserv" => {
                let service = match cmd {
                    "ns" | "nickserv" => "NickServ",
                    "cs" | "chanserv" => "ChanServ",
                    "ms" | "memoserv" => "MemoServ",
                    "os" | "operserv" => "OperServ",
                    "hs" | "hostserv" => "HostServ",
                    _ => "BotServ",
                };
                if rest.is_empty() {
                    out.push(self.error(buffer, format!("Usage : /{cmd} commande…"), now));
                } else {
                    self.say(service, rest, false, now, out);
                }
            }
            "quote" | "raw" | "send" => {
                if rest.is_empty() {
                    out.push(self.error(buffer, "Usage : /quote COMMANDE paramètres", now));
                } else {
                    let b = buffer.to_string();
                    out.push(self.line(&b, LineKind::Raw, "", format!("» {rest}"), now, false, true));
                    // Raw: the line goes as typed, only its terminators are removed.
                    out.push(Out::Send(rest.chars().filter(|c| !matches!(c, '\r' | '\n' | '\0')).collect()));
                }
            }
            "help" | "aide" | "?" => {
                for l in HELP {
                    out.push(self.info(buffer, *l, now));
                }
            }
            "clear" | "effacer" => {} // handled by the page
            _ => out.push(self.error(buffer, format!("Commande inconnue : /{cmd} (tapez /aide)"), now)),
        }
    }
}

const HELP: &[&str] = &[
    "Commandes : /join #salon [clé] · /part [#salon] [message] · /msg pseudo texte · /query pseudo · /me action",
    "            /nick pseudo · /away [message] · /whois pseudo · /topic [sujet] · /invite pseudo [#salon]",
    "            /kick pseudo [raison] · /ban pseudo · /op · /deop · /voice · /devoice · /mode [cible] +/-modes",
    "            /list [filtre] · /ns /cs /ms … (services) · /ctcp pseudo VERSION · /quote COMMANDE · /clear · /quit [message]",
    "Un texte sans / est envoyé au salon ou à la conversation de la fenêtre. // au début envoie un / littéral.",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SessionConfig {
        SessionConfig {
            nick: "kirc".into(),
            user: "kirc".into(),
            realname: "KIRC test".into(),
            quit_message: "Au revoir".into(),
            part_message: "".into(),
            version: "KIRC 0.1".into(),
            ..Default::default()
        }
    }

    fn feed(s: &mut Session, line: &str) -> Vec<Out> {
        s.handle(&Message::parse(line).unwrap(), 1_000)
    }

    fn sent(out: &[Out]) -> Vec<String> {
        out.iter()
            .filter_map(|o| match o {
                Out::Send(l) | Out::SendNow(l) => Some(l.clone()),
                _ => None,
            })
            .collect()
    }

    fn lines(out: &[Out]) -> Vec<Line> {
        out.iter()
            .filter_map(|o| match o {
                Out::Ui(Event::Line { line }) => Some(line.clone()),
                _ => None,
            })
            .collect()
    }

    fn events(out: &[Out]) -> Vec<Event> {
        out.iter().filter_map(|o| if let Out::Ui(e) = o { Some(e.clone()) } else { None }).collect()
    }

    fn registered(cfg: SessionConfig) -> Session {
        let mut s = Session::new(cfg);
        s.start();
        feed(&mut s, "CAP * LS :");
        feed(&mut s, ":srv 001 kirc :Bienvenue");
        s
    }

    fn joined(s: &mut Session, chan: &str) {
        feed(s, &format!(":kirc!k@h JOIN {chan}"));
        feed(s, &format!(":srv 353 kirc = {chan} :@op +voice plain"));
        feed(s, &format!(":srv 366 kirc {chan} :End of /NAMES list."));
    }

    #[test]
    fn fold_follows_rfc1459() {
        assert_eq!(fold("Nick[A]\\~"), "nick{a}|^");
        assert_eq!(fold("#Rust"), "#rust");
    }

    #[test]
    fn mentions_matches_whole_words_only() {
        assert!(mentions("salut kirc !", "kirc"));
        assert!(mentions("KIRC: tu es là ?", "kirc"));
        assert!(!mentions("kircher est là", "kirc"));
        assert!(!mentions("le_kirc_bot", "kirc"));
        assert!(!mentions("rien", ""));
        assert!(!mentions("x kirc-", "kirc"), "'-' is a nick character");
    }

    #[test]
    fn parses_server_time() {
        assert_eq!(parse_server_time("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_server_time("2026-10-28T12:00:00.500Z"), Some(1_793_188_800_500));
        assert_eq!(parse_server_time("2000-03-01T00:00:01Z"), Some(951_868_801_000));
        assert_eq!(parse_server_time("nope"), None);
    }

    #[test]
    fn registration_without_capabilities() {
        let mut s = Session::new(SessionConfig { password: Some("secret".into()), ..cfg() });
        let out = sent(&s.start());
        assert_eq!(out[0], "CAP LS 302");
        assert_eq!(out[1], "PASS secret");
        assert_eq!(out[2], "NICK kirc");
        assert_eq!(out[3], "USER kirc 0 * :KIRC test");
        // The server offers nothing we want: CAP END straight away.
        let out = sent(&feed(&mut s, ":srv CAP * LS :multi-prefix=x batch"));
        assert!(out.iter().any(|l| l.starts_with("CAP REQ :multi-prefix")));
        let out = sent(&feed(&mut s, ":srv CAP kirc ACK :multi-prefix"));
        assert_eq!(out, vec!["CAP END"]);
        // CAP END is never sent twice.
        assert!(sent(&feed(&mut s, ":srv CAP kirc NAK :x")).is_empty());
        assert!(!s.is_registered());
        let out = feed(&mut s, ":srv 001 kirc :Welcome");
        assert!(s.is_registered());
        assert!(events(&out).iter().any(|e| matches!(e, Event::Status { state: ConnState::Connected, .. })));
    }

    #[test]
    fn multiline_cap_ls_waits_for_the_last_line() {
        let mut s = Session::new(cfg());
        s.start();
        assert!(sent(&feed(&mut s, ":srv CAP * LS * :a b")).is_empty());
        let out = sent(&feed(&mut s, ":srv CAP * LS :away-notify"));
        assert_eq!(out, vec!["CAP REQ :away-notify"]);
    }

    #[test]
    fn sasl_plain_flow() {
        let mut s = Session::new(SessionConfig { sasl: Some(("bob".into(), "pw".into())), ..cfg() });
        s.start();
        let out = sent(&feed(&mut s, ":srv CAP * LS :sasl multi-prefix"));
        assert_eq!(out, vec!["CAP REQ :multi-prefix", "CAP REQ :sasl"]);
        assert!(sent(&feed(&mut s, ":srv CAP kirc ACK :multi-prefix")).is_empty(), "waits for the other answer");
        assert_eq!(sent(&feed(&mut s, ":srv CAP kirc ACK :sasl")), vec!["AUTHENTICATE PLAIN"]);
        let out = sent(&feed(&mut s, "AUTHENTICATE +"));
        assert_eq!(out.len(), 1);
        let payload = out[0].strip_prefix("AUTHENTICATE ").unwrap();
        let raw = base64::engine::general_purpose::STANDARD.decode(payload).unwrap();
        assert_eq!(raw, b"bob\0bob\0pw");
        assert_eq!(sent(&feed(&mut s, ":srv 903 kirc :SASL authentication successful")), vec!["CAP END"]);
    }

    #[test]
    fn a_refused_capability_does_not_cancel_the_others_and_a_refused_sasl_is_reported() {
        let mut s = Session::new(SessionConfig { sasl: Some(("bob".into(), "pw".into())), ..cfg() });
        s.start();
        feed(&mut s, ":srv CAP * LS :sasl=PLAIN,EXTERNAL multi-prefix");
        let out = feed(&mut s, ":srv CAP kirc NAK :sasl");
        assert!(lines(&out).iter().any(|l| l.kind == LineKind::Error && l.text.contains("SASL")));
        assert!(sent(&out).is_empty());
        assert_eq!(sent(&feed(&mut s, ":srv CAP kirc ACK :multi-prefix")), vec!["CAP END"]);
    }

    #[test]
    fn sasl_without_the_plain_mechanism_is_reported() {
        let mut s = Session::new(SessionConfig { sasl: Some(("bob".into(), "pw".into())), ..cfg() });
        s.start();
        let out = feed(&mut s, ":srv CAP * LS :sasl=EXTERNAL");
        assert!(lines(&out).iter().any(|l| l.kind == LineKind::Error && l.text.contains("SASL PLAIN")));
        assert_eq!(sent(&out), vec!["CAP END"]);
    }

    #[test]
    fn hostile_messages_create_no_buffers() {
        let mut s = registered(cfg());
        assert!(lines(&feed(&mut s, ":x!y@z PRIVMSG #nowhere :spam")).is_empty());
        assert!(lines(&feed(&mut s, "PRIVMSG kirc :sans expéditeur")).is_empty());
        let l = lines(&feed(&mut s, ":x!y@z NOTICE #nowhere :spam"));
        assert_eq!(l[0].buffer, "");
    }

    #[test]
    fn sasl_failure_is_reported_and_registration_continues() {
        let mut s = Session::new(SessionConfig {
            sasl: Some(("bob".into(), "bad".into())),
            nickserv_password: Some("bad".into()),
            ..cfg()
        });
        s.start();
        feed(&mut s, ":srv CAP * LS :sasl");
        feed(&mut s, ":srv CAP kirc ACK :sasl");
        feed(&mut s, "AUTHENTICATE +");
        let out = feed(&mut s, ":srv 904 kirc :SASL authentication failed");
        assert_eq!(sent(&out), vec!["CAP END"]);
        assert!(lines(&out).iter().any(|l| l.kind == LineKind::Error && l.text.contains("SASL")));
        // Without SASL, NickServ takes over after the welcome.
        let out = feed(&mut s, ":srv 001 kirc :Welcome");
        assert!(sent(&out).iter().any(|l| l == "PRIVMSG NickServ :IDENTIFY bad"));
    }

    #[test]
    fn nickserv_identify_delays_the_autojoin_until_confirmed() {
        let mut s = Session::new(SessionConfig {
            nickserv_password: Some("pw".into()),
            autojoin: vec!["#a".into(), "#b clé".into()],
            ..cfg()
        });
        s.start();
        feed(&mut s, "CAP * LS :");
        let out = sent(&feed(&mut s, ":srv 001 kirc :Welcome"));
        assert_eq!(out, vec!["PRIVMSG NickServ :IDENTIFY pw"]);
        assert!(s.tick(1_500).is_empty(), "not yet");
        feed(&mut s, ":NickServ!NickServ@services NOTICE kirc :You are now identified for kirc.");
        let out = sent(&s.tick(1_600));
        assert_eq!(out, vec!["JOIN #b clé", "JOIN #a"], "keyed channels first");
        assert!(s.tick(9_999).is_empty(), "only once");
    }

    #[test]
    fn autojoin_fires_after_the_timeout_without_a_confirmation() {
        let mut s = Session::new(SessionConfig {
            nickserv_password: Some("pw".into()),
            autojoin: vec!["#a".into()],
            ..cfg()
        });
        s.start();
        s.handle(&Message::parse(":srv 001 kirc :Welcome").unwrap(), 10_000);
        assert!(s.tick(12_000).is_empty());
        assert_eq!(sent(&s.tick(13_000)), vec!["JOIN #a"]);
    }

    #[test]
    fn nick_in_use_falls_back_to_alternatives_then_underscores() {
        let mut s = Session::new(SessionConfig { alt_nicks: vec!["kirc2".into()], ..cfg() });
        s.start();
        assert_eq!(sent(&feed(&mut s, ":srv 433 * kirc :Nickname is already in use")), vec!["NICK kirc2"]);
        assert_eq!(sent(&feed(&mut s, ":srv 433 * kirc2 :Nickname is already in use")), vec!["NICK kirc_"]);
        assert_eq!(sent(&feed(&mut s, ":srv 433 * kirc_ :Nickname is already in use")), vec!["NICK kirc__"]);
        feed(&mut s, ":srv 001 kirc__ :Welcome");
        assert_eq!(s.nick(), "kirc__");
    }

    #[test]
    fn answers_ping() {
        let mut s = registered(cfg());
        assert_eq!(sent(&feed(&mut s, "PING :token")), vec!["PONG token"]);
        assert_eq!(sent(&feed(&mut s, "PING :a token")), vec!["PONG :a token"]);
    }

    #[test]
    fn isupport_sets_prefixes_and_chantypes() {
        let mut s = registered(cfg());
        feed(&mut s, ":srv 005 kirc PREFIX=(qaohv)~&@%+ CHANTYPES=#& NETWORK=Libera CHANMODES=beI,k,l,imnpst :are supported");
        joined_with_names(&mut s);
    }

    fn joined_with_names(s: &mut Session) {
        feed(s, ":kirc!k@h JOIN #c");
        let out = feed(s, ":srv 353 kirc = #c :~owner &admin @op %half +voice @+both plain");
        assert!(events(&out).is_empty());
        let out = feed(s, ":srv 366 kirc #c :End");
        let Event::Users { users, .. } = &events(&out)[0] else { panic!("Users expected") };
        let get = |n: &str| users.iter().find(|u| u.nick == n).unwrap();
        assert_eq!(get("owner").modes, "~");
        assert_eq!(get("both").modes, "@+");
        assert_eq!(get("plain").modes, "");
        assert_eq!(users.len(), 7);
    }

    #[test]
    fn multi_prefix_is_ordered_by_rank() {
        let mut s = registered(cfg());
        feed(&mut s, ":kirc!k@h JOIN #c");
        feed(&mut s, ":srv 353 kirc = #c :+@bob");
        let out = feed(&mut s, ":srv 366 kirc #c :End");
        let Event::Users { users, .. } = &events(&out)[0] else { panic!() };
        assert_eq!(users[0].modes, "@+");
    }

    #[test]
    fn own_join_opens_the_window_and_others_update_the_list() {
        let mut s = registered(cfg());
        let out = feed(&mut s, ":kirc!k@h JOIN #Rust");
        let ev = events(&out);
        assert!(matches!(&ev[0], Event::Joined { buffer } if buffer == "#Rust"));
        let out = feed(&mut s, ":alice!a@h JOIN #rust");
        let ev = events(&out);
        assert!(matches!(&ev[0], Event::UserSet { buffer, user } if buffer == "#Rust" && user.nick == "alice"));
        assert_eq!(lines(&out)[0].buffer, "#Rust", "case of the first JOIN is kept");
        let out = feed(&mut s, ":alice!a@h PART #rust :bye");
        assert!(events(&out).iter().any(|e| matches!(e, Event::UserDel { nick, .. } if nick == "alice")));
        assert!(lines(&out)[0].text.contains("(bye)"));
    }

    #[test]
    fn quit_reaches_every_shared_channel_only() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        joined(&mut s, "#b");
        feed(&mut s, ":bob!b@h JOIN #a");
        let out = feed(&mut s, ":bob!b@h QUIT :Ping timeout");
        let l = lines(&out);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].buffer, "#a");
        assert_eq!(l[0].kind, LineKind::Quit);
    }

    #[test]
    fn nick_change_renames_members_and_queries() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        feed(&mut s, ":plain!p@h PRIVMSG kirc :coucou");
        let out = feed(&mut s, ":plain!p@h NICK :plain2");
        let ev = events(&out);
        assert!(ev.iter().any(|e| matches!(e, Event::UserDel { nick, .. } if nick == "plain")));
        assert!(ev.iter().any(|e| matches!(e, Event::UserSet { user, .. } if user.nick == "plain2")));
        assert!(ev.iter().any(|e| matches!(e, Event::Query { buffer } if buffer == "plain2")));
        // Our own nick.
        let out = feed(&mut s, ":kirc!k@h NICK :kirc_");
        assert_eq!(s.nick(), "kirc_");
        assert!(events(&out).iter().any(|e| matches!(e, Event::Me { nick, .. } if nick == "kirc_")));
    }

    #[test]
    fn mode_changes_update_member_ranks() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let out = feed(&mut s, ":op!o@h MODE #a +o plain");
        let Event::UserSet { user, .. } = &events(&out)[0] else { panic!() };
        assert_eq!((user.nick.as_str(), user.modes.as_str()), ("plain", "@"));
        let out = feed(&mut s, ":op!o@h MODE #a -ov+b plain voice *!*@bad");
        let ev = events(&out);
        assert!(ev.iter().any(|e| matches!(e, Event::UserSet { user, .. } if user.nick == "plain" && user.modes.is_empty())));
        assert!(ev.iter().any(|e| matches!(e, Event::UserSet { user, .. } if user.nick == "voice" && user.modes.is_empty())));
        // Mask parameters of +b must not shift the nick arguments of a later mode.
        let out = feed(&mut s, ":op!o@h MODE #a +bo *!*@x plain");
        let Event::UserSet { user, .. } = &events(&out)[0] else { panic!() };
        assert_eq!((user.nick.as_str(), user.modes.as_str()), ("plain", "@"));
    }

    #[test]
    fn kick_of_ourselves_closes_the_window() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let out = feed(&mut s, ":op!o@h KICK #a kirc :trop bruyant");
        assert!(events(&out).iter().any(|e| matches!(e, Event::Parted { buffer } if buffer == "#a")));
        let l = lines(&out);
        assert!(l[0].text.contains("vous a expulsé") && l[0].highlight);
        assert!(s.channel_names().is_empty());
    }

    #[test]
    fn channel_messages_highlight_only_on_mention() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let l = lines(&feed(&mut s, ":plain!p@h PRIVMSG #a :bonjour tout le monde"));
        assert!(!l[0].highlight);
        let l = lines(&feed(&mut s, ":plain!p@h PRIVMSG #a :kirc, tu es là ?"));
        assert!(l[0].highlight);
        assert_eq!(l[0].nick, "plain");
    }

    #[test]
    fn private_message_opens_a_query_once() {
        let mut s = registered(cfg());
        let out = feed(&mut s, ":Bob!b@h PRIVMSG kirc :salut");
        assert!(events(&out).iter().any(|e| matches!(e, Event::Query { buffer } if buffer == "Bob")));
        let l = lines(&out);
        assert_eq!((l[0].buffer.as_str(), l[0].highlight), ("Bob", true));
        let out = feed(&mut s, ":bob!b@h PRIVMSG kirc :encore");
        assert!(!events(&out).iter().any(|e| matches!(e, Event::Query { .. })));
        assert_eq!(lines(&out)[0].buffer, "Bob");
    }

    #[test]
    fn actions_and_ctcp() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let l = lines(&feed(&mut s, ":plain!p@h PRIVMSG #a :\u{1}ACTION danse\u{1}"));
        assert_eq!((l[0].kind, l[0].text.as_str()), (LineKind::Action, "danse"));
        let out = s.handle(&Message::parse(":plain!p@h PRIVMSG kirc :\u{1}VERSION\u{1}").unwrap(), 5_000);
        assert_eq!(sent(&out), vec!["NOTICE plain :\u{1}VERSION KIRC 0.1\u{1}"]);
        // Rate limit: a second request within a second is ignored.
        let out = s.handle(&Message::parse(":plain!p@h PRIVMSG kirc :\u{1}VERSION\u{1}").unwrap(), 5_500);
        assert!(sent(&out).is_empty());
        let out = s.handle(&Message::parse(":plain!p@h PRIVMSG kirc :\u{1}VERSION\u{1}").unwrap(), 6_500);
        assert_eq!(sent(&out).len(), 1);
        // Unknown requests get no answer.
        let out = s.handle(&Message::parse(":plain!p@h PRIVMSG kirc :\u{1}FINGER\u{1}").unwrap(), 20_000);
        assert!(sent(&out).is_empty());
    }

    #[test]
    fn server_notices_go_to_the_console_and_user_notices_to_the_right_place() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let l = lines(&feed(&mut s, ":srv.example.org NOTICE * :*** Looking up your hostname"));
        assert_eq!(l[0].buffer, "");
        let l = lines(&feed(&mut s, ":ChanServ!cs@services NOTICE kirc :bienvenue sur #a"));
        assert_eq!((l[0].buffer.as_str(), l[0].kind), ("", LineKind::Notice));
        let l = lines(&feed(&mut s, ":plain!p@h NOTICE #a :annonce"));
        assert_eq!(l[0].buffer, "#a");
    }

    #[test]
    fn topic_events() {
        let mut s = registered(cfg());
        feed(&mut s, ":kirc!k@h JOIN #a");
        let out = feed(&mut s, ":srv 332 kirc #a :Sujet du jour");
        assert!(events(&out).iter().any(|e| matches!(e, Event::Topic { topic, .. } if topic == "Sujet du jour")));
        let out = feed(&mut s, ":op!o@h TOPIC #a :Nouveau");
        assert!(events(&out).iter().any(|e| matches!(e, Event::Topic { topic, .. } if topic == "Nouveau")));
        assert!(lines(&out)[0].text.contains("a changé le sujet"));
    }

    #[test]
    fn away_notify_marks_members() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let out = feed(&mut s, ":plain!p@h AWAY :déjeuner");
        assert!(matches!(&events(&out)[0], Event::UserSet { user, .. } if user.away));
        let out = feed(&mut s, ":plain!p@h AWAY");
        assert!(matches!(&events(&out)[0], Event::UserSet { user, .. } if !user.away));
    }

    #[test]
    fn disconnect_clears_channels_and_rejoin_happens_after_the_next_welcome() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        joined(&mut s, "#b");
        let out = s.on_disconnect("Ping timeout", 2_000);
        assert!(lines(&out).iter().all(|l| l.kind == LineKind::Error && l.text.contains("Ping timeout")));
        assert!(events(&out).iter().any(|e| matches!(e, Event::Users { buffer, users } if buffer == "#a" && users.is_empty())));
        assert!(s.channel_names().is_empty());
        s.start();
        feed(&mut s, "CAP * LS :");
        let out = sent(&feed(&mut s, ":srv 001 kirc :Welcome"));
        assert_eq!(out, vec!["JOIN #a", "JOIN #b"]);
    }

    // --- typed input

    #[test]
    fn plain_text_goes_to_the_channel_and_is_echoed() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let out = s.user_input("#a", "bonjour", 1);
        assert_eq!(sent(&out), vec!["PRIVMSG #a bonjour"]);
        let l = lines(&out);
        assert!(l[0].own && l[0].kind == LineKind::Msg && l[0].nick == "kirc" && l[0].buffer == "#a");
        let out = s.user_input("#a", "bonjour à tous", 1);
        assert_eq!(sent(&out), vec!["PRIVMSG #a :bonjour à tous"]);
    }

    #[test]
    fn double_slash_sends_a_literal_slash() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        assert_eq!(sent(&s.user_input("#a", "//shrug", 1)), vec!["PRIVMSG #a /shrug"]);
    }

    #[test]
    fn console_refuses_plain_text() {
        let mut s = registered(cfg());
        let out = s.user_input("", "bonjour", 1);
        assert!(sent(&out).is_empty());
        assert_eq!(lines(&out)[0].kind, LineKind::Error);
    }

    #[test]
    fn long_text_is_split() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let long = "mot ".repeat(300);
        let out = s.user_input("#a", &long, 1);
        let wire = sent(&out);
        assert!(wire.len() >= 3);
        assert!(wire.iter().all(|l| l.len() + 2 <= 512), "every line fits the protocol limit");
    }

    #[test]
    fn join_command_adds_the_hash_and_handles_keys() {
        let mut s = registered(cfg());
        assert_eq!(sent(&s.user_input("", "/join rust", 1)), vec!["JOIN #rust"]);
        assert_eq!(sent(&s.user_input("", "/j #a,#b k1,k2", 1)), vec!["JOIN #a k1", "JOIN #b k2"]);
        assert_eq!(sent(&s.user_input("", "/join &local", 1)), vec!["JOIN &local"]);
    }

    #[test]
    fn part_uses_the_current_channel() {
        let mut s = registered(SessionConfig { part_message: "Salut".into(), ..cfg() });
        joined(&mut s, "#a");
        assert_eq!(sent(&s.user_input("#a", "/part", 1)), vec!["PART #a Salut"]);
        assert_eq!(sent(&s.user_input("#a", "/part #b à demain", 1)), vec!["PART #b :à demain"]);
        assert!(sent(&s.user_input("", "/part", 1)).is_empty());
    }

    #[test]
    fn msg_to_a_nick_opens_a_query_and_services_stay_in_the_console() {
        let mut s = registered(cfg());
        let out = s.user_input("", "/msg bob salut toi", 1);
        assert_eq!(sent(&out), vec!["PRIVMSG bob :salut toi"]);
        assert!(events(&out).iter().any(|e| matches!(e, Event::Query { buffer } if buffer == "bob")));
        assert_eq!(lines(&out)[0].buffer, "bob");
        let out = s.user_input("#x", "/ns identify monmotdepasse", 1);
        assert_eq!(sent(&out), vec!["PRIVMSG NickServ :identify monmotdepasse"]);
        let l = lines(&out);
        assert_eq!(l[0].buffer, "");
        assert!(!l[0].text.contains("monmotdepasse"), "the password is never echoed: {}", l[0].text);
        assert!(!events(&out).iter().any(|e| matches!(e, Event::Query { .. })));
    }

    #[test]
    fn me_sends_a_ctcp_action() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let out = s.user_input("#a", "/me mange", 1);
        assert_eq!(sent(&out), vec!["PRIVMSG #a :\u{1}ACTION mange\u{1}"]);
        assert_eq!(lines(&out)[0].kind, LineKind::Action);
    }

    #[test]
    fn moderation_commands() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        assert_eq!(sent(&s.user_input("#a", "/op bob alice", 1)), vec!["MODE #a +oo bob alice"]);
        assert_eq!(sent(&s.user_input("#a", "/devoice bob", 1)), vec!["MODE #a -v bob"]);
        assert_eq!(sent(&s.user_input("#a", "/kick bob trop de bruit", 1)), vec!["KICK #a bob :trop de bruit"]);
        assert_eq!(sent(&s.user_input("#a", "/ban bob", 1)), vec!["MODE #a +b bob!*@*"]);
        assert_eq!(sent(&s.user_input("#a", "/mode +m", 1)), vec!["MODE #a +m"]);
        assert_eq!(sent(&s.user_input("#a", "/mode #b +t", 1)), vec!["MODE #b +t"]);
        assert_eq!(sent(&s.user_input("#a", "/topic", 1)), vec!["TOPIC #a"]);
        assert_eq!(sent(&s.user_input("#a", "/topic Un sujet", 1)), vec!["TOPIC #a :Un sujet"]);
        assert!(sent(&s.user_input("", "/kick bob", 1)).is_empty(), "needs a channel");
    }

    #[test]
    fn many_op_targets_are_grouped_by_four() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let wire = sent(&s.user_input("#a", "/op a b c d e", 1));
        assert_eq!(wire, vec!["MODE #a +oooo a b c d", "MODE #a +o e"]);
    }

    #[test]
    fn quit_command_sends_quit_and_asks_to_close() {
        let mut s = registered(cfg());
        let out = s.user_input("", "/quit", 1);
        assert_eq!(sent(&out), vec!["QUIT :Au revoir"]);
        assert!(out.iter().any(|o| matches!(o, Out::Quit)));
        assert!(s.is_quitting());
    }

    #[test]
    fn quote_cannot_inject_a_second_line() {
        let mut s = registered(cfg());
        let out = sent(&s.user_input("", "/quote PING x\r\nQUIT :pwned", 1));
        assert_eq!(out, vec!["PING xQUIT :pwned"]);
    }

    #[test]
    fn unknown_commands_are_reported() {
        let mut s = registered(cfg());
        let out = s.user_input("", "/blorp", 1);
        assert!(sent(&out).is_empty());
        assert!(lines(&out)[0].text.contains("Commande inconnue"));
    }

    #[test]
    fn whois_is_collected_into_one_card() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        s.user_input("#a", "/whois bob", 1);
        let mut shown = Vec::new();
        for l in [
            ":srv 311 kirc bob ~b host.example * :Bob Martin",
            ":srv 312 kirc bob irc.example.org :Le serveur",
            ":srv 301 kirc bob :parti déjeuner",
            ":srv 313 kirc bob :is an IRC operator",
            ":srv 317 kirc bob 42 1700000000 :seconds idle, signon time",
            ":srv 319 kirc bob :@#a +#b",
            ":srv 330 kirc bob bobaccount :is logged in as",
            ":srv 671 kirc bob :is using a secure connection",
        ] {
            shown.extend(feed(&mut s, l));
        }
        assert!(shown.is_empty(), "nothing is shown before the end of the list");
        let out = feed(&mut s, ":srv 318 kirc bob :End of /WHOIS list.");
        let Event::Whois { info } = &events(&out)[0] else { panic!("Whois expected") };
        assert_eq!((info.nick.as_str(), info.user.as_str(), info.host.as_str()), ("bob", "~b", "host.example"));
        assert_eq!(info.realname, "Bob Martin");
        assert_eq!((info.server.as_str(), info.idle, info.signon), ("irc.example.org", Some(42), Some(1_700_000_000)));
        assert_eq!((info.channels.as_str(), info.account.as_str(), info.away.as_str()), ("@#a +#b", "bobaccount", "parti déjeuner"));
        assert!(info.secure && info.operator);
        // Outside a WHOIS, an away reply is an ordinary line.
        let l = lines(&feed(&mut s, ":srv 301 kirc bob :absent"));
        assert!(l[0].text.contains("est absent"));
    }

    #[test]
    fn channel_list_is_collected_and_capped() {
        let mut s = registered(cfg());
        let out = feed(&mut s, ":srv 321 kirc Channel :Users  Name");
        assert!(matches!(events(&out)[0], Event::ListStart));
        feed(&mut s, ":srv 322 kirc #a 120 :[+nt] le salon A");
        feed(&mut s, ":srv 322 kirc #b 3 :");
        feed(&mut s, ":srv 322 kirc #c notanumber :x");
        let out = feed(&mut s, ":srv 323 kirc :End of /LIST");
        let Event::ChannelList { entries } = &events(&out)[0] else { panic!("ChannelList expected") };
        assert_eq!(entries.len(), 3);
        assert_eq!((entries[0].name.as_str(), entries[0].users, entries[0].topic.as_str()), ("#a", 120, "[+nt] le salon A"));
        assert_eq!(entries[2].users, 0);
        // Rows without a start are ignored, a flood is bounded.
        assert!(events(&feed(&mut s, ":srv 323 kirc :End of /LIST")).is_empty());
        feed(&mut s, ":srv 321 kirc Channel :Users  Name");
        for i in 0..(MAX_LIST + 50) {
            feed(&mut s, &format!(":srv 322 kirc #c{i} 1 :t"));
        }
        let out = feed(&mut s, ":srv 323 kirc :End of /LIST");
        let Event::ChannelList { entries } = &events(&out)[0] else { panic!() };
        assert_eq!(entries.len(), MAX_LIST);
        assert_eq!(sent(&s.user_input("", "/list", 1)), vec!["LIST"]);
        assert_eq!(sent(&s.user_input("", "/list >100", 1)), vec!["LIST >100"]);
    }

    #[test]
    fn glob_and_masks() {
        assert!(glob_match("b*b", "bob"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a?c", "abc") && !glob_match("a?c", "ac"));
        assert!(glob_match("*!*@*.example.org", "bob!~b@host.example.org"));
        assert!(!glob_match("bob!*@*", "bobby!x@y"));
        assert!(glob_match("*a*a*a*", "banana") && !glob_match("*a*a*a*a", "banana"));
        assert_eq!(mask_of("Bob"), "bob!*@*");
        assert_eq!(mask_of("*@Host"), "*!*@host");
        assert_eq!(mask_of("A!b@c"), "a!b@c");
    }

    #[test]
    fn ignored_people_are_silent_everywhere() {
        let mut s = registered(SessionConfig { ignores: vec!["Spammer".into(), "*@bad.host".into()], ..cfg() });
        joined(&mut s, "#a");
        assert!(feed(&mut s, ":spammer!x@y PRIVMSG #a :achetez").is_empty());
        assert!(feed(&mut s, ":other!x@bad.host PRIVMSG kirc :psst").is_empty(), "no query is opened either");
        assert!(feed(&mut s, ":Spammer!x@y NOTICE kirc :promo").is_empty());
        assert!(feed(&mut s, ":spammer!x@y PRIVMSG #a :\u{1}ACTION danse\u{1}").is_empty());
        assert_eq!(lines(&feed(&mut s, ":friend!x@y PRIVMSG #a :salut")).len(), 1);
        s.set_filters(vec![], vec![]);
        assert_eq!(lines(&feed(&mut s, ":spammer!x@y PRIVMSG #a :de retour")).len(), 1);
    }

    #[test]
    fn alert_words_count_as_mentions() {
        let mut s = registered(SessionConfig { alert_words: vec!["Rust".into()], ..cfg() });
        joined(&mut s, "#a");
        assert!(lines(&feed(&mut s, ":p!x@y PRIVMSG #a :j'adore rust !")).first().unwrap().highlight);
        assert!(!lines(&feed(&mut s, ":p!x@y PRIVMSG #a :trustworthy")).first().unwrap().highlight);
        s.set_filters(vec![], vec![]);
        assert!(!lines(&feed(&mut s, ":p!x@y PRIVMSG #a :rust")).first().unwrap().highlight);
    }

    #[test]
    fn error_numerics_are_shown() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        s.user_input("#a", "/kick bob", 1);
        let l = lines(&feed(&mut s, ":srv 482 kirc #a :You're not channel operator"));
        assert_eq!((l[0].kind, l[0].buffer.as_str()), (LineKind::Error, "#a"));
        assert!(l[0].text.contains("not channel operator"));
    }

    #[test]
    fn server_time_tag_is_used_for_the_line_time() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let l = lines(&feed(&mut s, "@time=2026-10-28T12:00:00.000Z :plain!p@h PRIVMSG #a :ancien message"));
        assert_eq!(l[0].time, 1_793_188_800_000);
    }

    #[test]
    fn a_channel_left_while_offline_is_not_rejoined() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        joined(&mut s, "#b");
        s.on_disconnect("Ping timeout", 2_000);
        s.forget_channel("#A");
        s.start();
        feed(&mut s, "CAP * LS :");
        assert_eq!(sent(&feed(&mut s, ":srv 001 kirc :Welcome")), vec!["JOIN #b"]);
    }

    #[test]
    fn username_with_spaces_is_sanitised() {
        let mut s = Session::new(SessionConfig { user: "a b@c".into(), ..cfg() });
        let out = sent(&s.start());
        assert_eq!(out.last().unwrap(), "USER abc 0 * :KIRC test");
    }
}
