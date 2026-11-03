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

/// A message identifier is short and plain: it goes into a tag, so nothing else is accepted.
pub fn valid_msgid(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-._~+/=:".contains(c))
}

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
    pending_reply: Option<(String, String)>,
    whois: Option<WhoisInfo>,
    list: Option<Vec<ChannelEntry>>,
    /// Where the answer to the last typed command should be shown.
    reply_buffer: String,
    away: bool,
    quitting: bool,
}

pub use crate::chat::fold;

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
            pending_reply: None,
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

    fn with_ids(mut o: Out, msgid: &str, reply_to: &str) -> Out {
        if let Out::Ui(Event::Line { line }) = &mut o {
            line.msgid = msgid.to_string();
            line.reply_to = reply_to.to_string();
        }
        o
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
            line: Line { buffer: buffer.to_string(), kind, nick: nick.to_string(), text, time, highlight, own, msgid: String::new(), reply_to: String::new(), media: None },
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
}

mod cap;
mod commands;
mod numeric;
mod privmsg;


#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SessionConfig {
        SessionConfig {
            nick: "kirc".into(),
            user: "kirc".into(),
            realname: "Messages test".into(),
            quit_message: "Au revoir".into(),
            part_message: "".into(),
            version: "Messages 0.1".into(),
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
        assert!(!mentions("x messages-", "kirc"), "'-' is a nick character");
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
        assert_eq!(out[3], "USER kirc 0 * :Messages test");
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
        feed(&mut s, ":NickServ!NickServ@services NOTICE kirc :You are now identified for messages.");
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
        assert_eq!(sent(&out), vec!["NOTICE plain :\u{1}VERSION Messages 0.1\u{1}"]);
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

    fn registered_with_tags() -> Session {
        let mut s = Session::new(cfg());
        s.start();
        feed(&mut s, ":srv CAP * LS :message-tags multi-prefix");
        feed(&mut s, ":srv CAP kirc ACK :message-tags");
        feed(&mut s, ":srv CAP kirc ACK :multi-prefix");
        feed(&mut s, ":srv 001 kirc :Welcome");
        joined(&mut s, "#a");
        s
    }

    #[test]
    fn an_answer_goes_out_with_the_reply_tag_when_the_server_has_message_tags() {
        let mut s = registered_with_tags();
        let out = s.user_input_reply("#a", "oui, d'accord", Some(("m-42".into(), "bob".into())), 1);
        assert_eq!(sent(&out), vec!["@+draft/reply=m-42 PRIVMSG #a :oui, d'accord"]);
        let l = lines(&out);
        assert_eq!((l[0].reply_to.as_str(), l[0].text.as_str(), l[0].own), ("m-42", "oui, d'accord", true));
        // Only plain text answers: a command, an action and the next line do not.
        assert_eq!(sent(&s.user_input_reply("#a", "/me rit", Some(("m-42".into(), "bob".into())), 1)), vec!["PRIVMSG #a :\u{1}ACTION rit\u{1}"]);
        assert_eq!(sent(&s.user_input("#a", "suite", 1)), vec!["PRIVMSG #a suite"]);
    }

    #[test]
    fn without_message_tags_the_answer_starts_with_the_nick() {
        let mut s = registered(cfg());
        joined(&mut s, "#a");
        let out = s.user_input_reply("#a", "oui", Some(("m-42".into(), "bob".into())), 1);
        assert_eq!(sent(&out), vec!["PRIVMSG #a :bob: oui"]);
        assert_eq!(lines(&out)[0].reply_to, "");
        assert_eq!(lines(&out)[0].text, "bob: oui");
    }

    #[test]
    fn a_bad_identifier_or_a_service_never_gets_a_tag() {
        let mut s = registered_with_tags();
        for id in ["a b", "x;y", "", "\r\nQUIT", &"z".repeat(65)] {
            let out = s.user_input_reply("#a", "texte", Some((id.to_string(), "bob".into())), 1);
            assert_eq!(sent(&out), vec!["PRIVMSG #a texte"], "{id:?}");
        }
        let out = s.user_input_reply("", "rien", Some(("m1".into(), "bob".into())), 1);
        assert!(sent(&out).is_empty(), "the console has no conversation");
        // A reply to a private message works the same way.
        let out = s.user_input_reply("bob", "salut", Some(("m9".into(), "bob".into())), 1);
        assert_eq!(sent(&out), vec!["@+draft/reply=m9 PRIVMSG bob salut"]);
    }

    #[test]
    fn long_answers_tag_only_the_first_line() {
        let mut s = registered_with_tags();
        let out = sent(&s.user_input_reply("#a", &"mot ".repeat(300), Some(("m1".into(), "bob".into())), 1));
        assert!(out.len() >= 3);
        assert!(out[0].starts_with("@+draft/reply=m1 PRIVMSG #a "));
        assert!(out[1..].iter().all(|l| l.starts_with("PRIVMSG #a ")));
    }

    #[test]
    fn incoming_ids_and_replies_are_read_and_checked() {
        let mut s = registered_with_tags();
        let l = lines(&feed(&mut s, "@msgid=abc123;+draft/reply=zzz9 :bob!b@h PRIVMSG #a :re : salut"));
        assert_eq!((l[0].msgid.as_str(), l[0].reply_to.as_str()), ("abc123", "zzz9"));
        let l = lines(&feed(&mut s, "@msgid=m2;+draft/reply=old :bob!b@h PRIVMSG #a :\u{1}ACTION approuve\u{1}"));
        assert_eq!((l[0].kind, l[0].reply_to.as_str()), (LineKind::Action, "old"));
        // A value that is not a plain identifier is dropped; no tags, no ids.
        let long = "q".repeat(70);
        let l = lines(&feed(&mut s, &format!("@msgid={long};+draft/reply=a\\sb :bob!b@h PRIVMSG #a :x")));
        assert_eq!((l[0].msgid.as_str(), l[0].reply_to.as_str()), ("", ""));
        let l = lines(&feed(&mut s, ":bob!b@h PRIVMSG #a :sans étiquettes"));
        assert_eq!((l[0].msgid.as_str(), l[0].reply_to.as_str()), ("", ""));
    }

    #[test]
    fn username_with_spaces_is_sanitised() {
        let mut s = Session::new(SessionConfig { user: "a b@c".into(), ..cfg() });
        let out = sent(&s.start());
        assert_eq!(out.last().unwrap(), "USER abc 0 * :Messages test");
    }
}
