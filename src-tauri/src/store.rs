//! What each window needs to draw itself when it opens: the connection state
//! and, per buffer, the topic, the member list and the recent lines. It is fed
//! by the same events as the pages, so both always agree.

use crate::irc::event::{ConnState, Event, Line, UserEntry};
use crate::irc::session::fold;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};

#[derive(Serialize, Clone, Debug, Default)]
pub struct BufferState {
    pub name: String,
    pub topic: String,
    pub users: Vec<UserEntry>,
    pub lines: Vec<Line>,
    /// A channel we are on (as opposed to a private conversation).
    pub joined: bool,
}

/// Buffers kept per server (the console, joined channels, conversations).
const MAX_BUFFERS: usize = 300;

#[derive(Default)]
struct Buf {
    name: String,
    topic: String,
    users: HashMap<String, UserEntry>,
    lines: VecDeque<Line>,
    joined: bool,
}

pub struct ServerState {
    pub status: ConnState,
    pub status_text: String,
    pub nick: String,
    pub network: String,
    pub away: bool,
    /// Counts the events applied: a page that fetched a snapshot ignores older events.
    pub seq: u64,
    buffers: HashMap<String, Buf>,
    scrollback: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct ServerSummary {
    pub status: ConnState,
    pub status_text: String,
    pub nick: String,
    pub network: String,
    pub away: bool,
    pub channels: Vec<String>,
    /// Private conversations that exist (a window may or may not be open for them).
    pub queries: Vec<String>,
}

impl ServerState {
    pub fn new(scrollback: usize) -> Self {
        ServerState {
            status: ConnState::Disconnected,
            status_text: String::new(),
            nick: String::new(),
            network: String::new(),
            away: false,
            seq: 0,
            buffers: HashMap::new(),
            scrollback,
        }
    }

    pub fn set_scrollback(&mut self, n: usize) {
        self.scrollback = n;
        for b in self.buffers.values_mut() {
            while b.lines.len() > n {
                b.lines.pop_front();
            }
        }
    }

    pub fn summary(&self) -> ServerSummary {
        let mut channels: Vec<String> = self.buffers.values().filter(|b| b.joined).map(|b| b.name.clone()).collect();
        channels.sort_by_key(|c| fold(c));
        let mut queries: Vec<String> = self
            .buffers
            .values()
            .filter(|b| !b.joined && !b.name.is_empty() && !b.name.starts_with(['#', '&', '+', '!']))
            .map(|b| b.name.clone())
            .collect();
        queries.sort_by_key(|q| fold(q));
        ServerSummary {
            queries,
            status: self.status,
            status_text: self.status_text.clone(),
            nick: self.nick.clone(),
            network: self.network.clone(),
            away: self.away,
            channels,
        }
    }

    fn buf(&mut self, name: &str) -> &mut Buf {
        let key = fold(name);
        if self.buffers.len() >= MAX_BUFFERS && !self.buffers.contains_key(&key) {
            // Forget a conversation that is not a joined channel (nor the console) to make room.
            if let Some(old) = self.buffers.iter().find(|(k, b)| !b.joined && !k.is_empty()).map(|(k, _)| k.clone()) {
                self.buffers.remove(&old);
            }
        }
        let b = self.buffers.entry(key).or_default();
        if b.name.is_empty() {
            b.name = name.to_string();
        }
        b
    }

    pub fn snapshot(&self, name: &str) -> BufferState {
        match self.buffers.get(&fold(name)) {
            Some(b) => {
                let mut users: Vec<UserEntry> = b.users.values().cloned().collect();
                users.sort_by_key(|a| fold(&a.nick));
                BufferState {
                    name: b.name.clone(),
                    topic: b.topic.clone(),
                    users,
                    lines: b.lines.iter().cloned().collect(),
                    joined: b.joined,
                }
            }
            None => BufferState { name: name.to_string(), ..Default::default() },
        }
    }

    pub fn clear_lines(&mut self, name: &str) {
        if let Some(b) = self.buffers.get_mut(&fold(name)) {
            b.lines.clear();
        }
    }

    pub fn apply(&mut self, ev: &Event) {
        self.seq += 1;
        match ev {
            Event::Status { state, text } => {
                self.status = *state;
                self.status_text = text.clone();
            }
            Event::Line { line } => {
                let cap = self.scrollback;
                let b = self.buf(&line.buffer);
                b.lines.push_back(line.clone());
                while b.lines.len() > cap {
                    b.lines.pop_front();
                }
            }
            Event::Users { buffer, users } => {
                let b = self.buf(buffer);
                b.users = users.iter().map(|u| (fold(&u.nick), u.clone())).collect();
            }
            Event::UserSet { buffer, user } => {
                self.buf(buffer).users.insert(fold(&user.nick), user.clone());
            }
            Event::UserDel { buffer, nick } => {
                self.buf(buffer).users.remove(&fold(nick));
            }
            Event::Topic { buffer, topic } => self.buf(buffer).topic = topic.clone(),
            Event::Joined { buffer } => {
                let b = self.buf(buffer);
                b.joined = true;
                b.users.clear();
                b.topic.clear();
            }
            Event::Parted { buffer } => {
                let b = self.buf(buffer);
                b.joined = false;
                b.users.clear();
            }
            Event::Query { buffer } => {
                self.buf(buffer);
            }
            Event::Me { nick, network } => {
                self.nick = nick.clone();
                if !network.is_empty() {
                    self.network = network.clone();
                }
            }
            Event::Away { away } => self.away = *away,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::irc::event::LineKind;

    fn line(buffer: &str, text: &str) -> Event {
        Event::Line {
            line: Line { buffer: buffer.into(), kind: LineKind::Msg, nick: "a".into(), text: text.into(), time: 0, highlight: false, own: false },
        }
    }

    #[test]
    fn scrollback_is_bounded_per_buffer() {
        let mut s = ServerState::new(3);
        for i in 0..10 {
            s.apply(&line("#a", &i.to_string()));
        }
        s.apply(&line("#b", "x"));
        let snap = s.snapshot("#A");
        assert_eq!(snap.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["7", "8", "9"]);
        assert_eq!(s.snapshot("#b").lines.len(), 1);
        s.set_scrollback(1);
        assert_eq!(s.snapshot("#a").lines.len(), 1);
    }

    #[test]
    fn users_topic_and_membership() {
        let mut s = ServerState::new(100);
        s.apply(&Event::Joined { buffer: "#Rust".into() });
        s.apply(&Event::UserSet { buffer: "#rust".into(), user: UserEntry { nick: "Zed".into(), modes: "@".into(), away: false } });
        s.apply(&Event::UserSet { buffer: "#rust".into(), user: UserEntry { nick: "alice".into(), modes: "".into(), away: false } });
        s.apply(&Event::Topic { buffer: "#rust".into(), topic: "Bienvenue".into() });
        let snap = s.snapshot("#RUST");
        assert_eq!(snap.name, "#Rust");
        assert_eq!(snap.users.iter().map(|u| u.nick.as_str()).collect::<Vec<_>>(), vec!["alice", "Zed"]);
        assert_eq!(snap.topic, "Bienvenue");
        s.apply(&Event::UserDel { buffer: "#rust".into(), nick: "ZED".into() });
        assert_eq!(s.snapshot("#rust").users.len(), 1);
        assert_eq!(s.summary().channels, vec!["#Rust"]);
        s.apply(&Event::Parted { buffer: "#rust".into() });
        assert!(s.summary().channels.is_empty());
        assert!(s.snapshot("#rust").users.is_empty());
    }

    #[test]
    fn unknown_buffer_gives_an_empty_snapshot() {
        let s = ServerState::new(10);
        let snap = s.snapshot("#nope");
        assert_eq!(snap.name, "#nope");
        assert!(snap.lines.is_empty() && !snap.joined);
    }

    #[test]
    fn status_and_identity() {
        let mut s = ServerState::new(10);
        s.apply(&Event::Status { state: ConnState::Connected, text: "ok".into() });
        s.apply(&Event::Me { nick: "kirc".into(), network: "Libera".into() });
        s.apply(&Event::Me { nick: "kirc2".into(), network: "".into() });
        s.apply(&Event::Away { away: true });
        let sum = s.summary();
        assert_eq!((sum.nick.as_str(), sum.network.as_str(), sum.away), ("kirc2", "Libera", true));
        assert_eq!(sum.status, ConnState::Connected);
    }

    #[test]
    fn clearing_keeps_the_members() {
        let mut s = ServerState::new(10);
        s.apply(&Event::Joined { buffer: "#a".into() });
        s.apply(&Event::UserSet { buffer: "#a".into(), user: UserEntry { nick: "x".into(), modes: "".into(), away: false } });
        s.apply(&line("#a", "hello"));
        s.clear_lines("#a");
        let snap = s.snapshot("#a");
        assert!(snap.lines.is_empty());
        assert_eq!(snap.users.len(), 1);
    }
}

#[cfg(test)]
mod query_tests {
    use super::*;

    #[test]
    fn summary_lists_private_conversations_but_not_the_console_or_left_channels() {
        let mut s = ServerState::new(10);
        s.apply(&Event::Status { state: ConnState::Connected, text: String::new() });
        s.apply(&Event::Query { buffer: "Bob".into() });
        s.apply(&Event::Joined { buffer: "#a".into() });
        s.apply(&Event::Parted { buffer: "#a".into() });
        s.apply(&Event::Joined { buffer: "#b".into() });
        let line = Line { buffer: String::new(), kind: crate::irc::event::LineKind::Info, nick: String::new(), text: "x".into(), time: 0, highlight: false, own: false };
        s.apply(&Event::Line { line });
        let sum = s.summary();
        assert_eq!(sum.queries, vec!["Bob"]);
        assert_eq!(sum.channels, vec!["#b"]);
    }
}
