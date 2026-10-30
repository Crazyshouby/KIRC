//! What each window needs to draw itself when it opens: the connection state
//! and, per buffer, the topic, the member list and the recent lines. It is fed
//! by the same events as the pages, so both always agree.

use crate::chat::event::{CallInfo, CallState, ChannelEntry, ConnState, Event, Line, PairingCode, UserEntry, WhoisInfo};
use crate::chat::fold;
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
    unread: u32,
    mentions: u32,
}

pub struct ServerState {
    pub status: ConnState,
    pub status_text: String,
    pub nick: String,
    pub network: String,
    pub away: bool,
    /// The code to scan to link the account, while there is one.
    pub pairing: Option<PairingCode>,
    /// The call in progress, if any.
    pub call: Option<CallInfo>,
    /// Counts the events applied: a page that fetched a snapshot ignores older events.
    pub seq: u64,
    channel_list: Vec<ChannelEntry>,
    list_loading: bool,
    whois: HashMap<String, WhoisInfo>,
    buffers: HashMap<String, Buf>,
    scrollback: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct Activity {
    pub name: String,
    pub unread: u32,
    pub mentions: u32,
}

/// One page of the channel list, filtered and sorted.
#[derive(Serialize, Clone, Debug)]
pub struct ListPage {
    /// Rows received.
    pub total: usize,
    /// Rows that match the filter.
    pub matched: usize,
    pub loading: bool,
    pub entries: Vec<ChannelEntry>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ServerSummary {
    pub status: ConnState,
    pub status_text: String,
    pub nick: String,
    pub network: String,
    pub away: bool,
    pub pairing: Option<PairingCode>,
    pub call: Option<CallInfo>,
    pub channels: Vec<String>,
    /// Buffers with lines nobody looked at yet.
    pub activity: Vec<Activity>,
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
            pairing: None,
            call: None,
            seq: 0,
            channel_list: Vec::new(),
            list_loading: false,
            whois: HashMap::new(),
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
        let mut activity: Vec<Activity> = self
            .buffers
            .values()
            .filter(|b| b.unread > 0 && !b.name.is_empty())
            .map(|b| Activity { name: b.name.clone(), unread: b.unread, mentions: b.mentions })
            .collect();
        activity.sort_by_key(|a| fold(&a.name));
        ServerSummary {
            activity,
            queries,
            status: self.status,
            status_text: self.status_text.clone(),
            nick: self.nick.clone(),
            network: self.network.clone(),
            away: self.away,
            pairing: self.pairing.clone(),
            call: self.call.clone(),
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

    /// A line arrived in a window nobody is looking at.
    pub fn bump(&mut self, name: &str, highlight: bool) {
        let b = self.buf(name);
        b.unread = b.unread.saturating_add(1);
        if highlight {
            b.mentions = b.mentions.saturating_add(1);
        }
    }

    pub fn whois(&self, nick: &str) -> Option<WhoisInfo> {
        self.whois.get(&fold(nick)).cloned()
    }

    /// Filters (case-insensitive text in the name or topic, minimum members) and sorts the list.
    pub fn list_page(&self, filter: &str, min_users: u32, by_name: bool, offset: usize, limit: usize) -> ListPage {
        let f = filter.trim().to_lowercase();
        let mut rows: Vec<&ChannelEntry> = self
            .channel_list
            .iter()
            .filter(|e| e.users >= min_users && (f.is_empty() || e.name.to_lowercase().contains(&f) || e.topic.to_lowercase().contains(&f)))
            .collect();
        if by_name {
            rows.sort_by_key(|e| fold(&e.name));
        } else {
            rows.sort_by(|a, b| b.users.cmp(&a.users).then_with(|| fold(&a.name).cmp(&fold(&b.name))));
        }
        ListPage {
            total: self.channel_list.len(),
            matched: rows.len(),
            loading: self.list_loading,
            entries: rows.into_iter().skip(offset).take(limit.min(500)).cloned().collect(),
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
                if *state == ConnState::Disconnected {
                    self.pairing = None;
                    self.call = None;
                }
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
            // a group (named like a channel) is "joined"; a private chat is just known
            Event::Listed { buffer } => {
                let channel = buffer.starts_with(['#', '&', '+', '!']);
                let b = self.buf(buffer);
                b.joined |= channel;
            }
            Event::Backlog { .. } => {}
            Event::Pairing { code } => self.pairing = Some(code.clone()),
            Event::Paired => self.pairing = None,
            Event::Call { call } => self.call = (call.state != CallState::Ended).then(|| call.clone()),
            Event::Me { nick, network } => {
                self.nick = nick.clone();
                if !network.is_empty() {
                    self.network = network.clone();
                }
            }
            Event::Away { away } => self.away = *away,
            Event::Whois { info } => {
                if self.whois.len() >= 20 {
                    self.whois.clear();
                }
                self.whois.insert(fold(&info.nick), (**info).clone());
            }
            Event::ListStart => {
                self.list_loading = true;
                self.channel_list.clear();
            }
            Event::ChannelList { entries } => {
                self.channel_list = entries.clone();
                self.list_loading = false;
            }
            Event::ListReady { .. } => {}
            Event::Read { buffer } => {
                if let Some(b) = self.buffers.get_mut(&fold(buffer)) {
                    b.unread = 0;
                    b.mentions = 0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::event::LineKind;

    fn line(buffer: &str, text: &str) -> Event {
        Event::Line {
            line: Line { buffer: buffer.into(), kind: LineKind::Msg, nick: "a".into(), text: text.into(), time: 0, highlight: false, own: false, msgid: String::new(), reply_to: String::new(), media: None },
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
    fn a_listed_chat_is_in_the_menu_without_losing_its_members() {
        let mut s = ServerState::new(100);
        s.apply(&Event::UserSet { buffer: "#Famille".into(), user: UserEntry { nick: "Alice".into(), modes: "".into(), away: false } });
        s.apply(&Event::Listed { buffer: "#Famille".into() });
        assert_eq!(s.summary().channels, vec!["#Famille"]);
        assert_eq!(s.snapshot("#famille").users.len(), 1, "Listed does not clear the members, unlike Joined");
    }

    #[test]
    fn a_call_is_kept_until_it_ends_or_the_link_drops() {
        let mut s = ServerState::new(100);
        let call = |state| CallInfo { id: "c1".into(), buffer: "Alice".into(), state, video: false, text: "Alice".into() };
        s.apply(&Event::Call { call: call(CallState::Incoming) });
        assert_eq!(s.summary().call.map(|c| c.state), Some(CallState::Incoming));
        s.apply(&Event::Call { call: call(CallState::Active) });
        assert_eq!(s.summary().call.map(|c| c.state), Some(CallState::Active));
        s.apply(&Event::Call { call: call(CallState::Ended) });
        assert!(s.summary().call.is_none());
        s.apply(&Event::Call { call: call(CallState::Calling) });
        s.apply(&Event::Status { state: ConnState::Disconnected, text: String::new() });
        assert!(s.summary().call.is_none(), "a dropped connection ends the call");
    }

    #[test]
    fn a_listed_private_chat_is_a_conversation_not_a_channel() {
        let mut s = ServerState::new(100);
        s.apply(&Event::Listed { buffer: "Alice".into() });
        let sum = s.summary();
        assert_eq!(sum.queries, vec!["Alice"]);
        assert!(sum.channels.is_empty());
    }

    #[test]
    fn the_pairing_code_lives_until_linking_ends_or_the_link_drops() {
        let mut s = ServerState::new(100);
        let code = PairingCode { size: 2, modules: "1001".into(), seconds: 60 };
        s.apply(&Event::Pairing { code: code.clone() });
        assert_eq!(s.summary().pairing, Some(code.clone()));
        s.apply(&Event::Paired);
        assert_eq!(s.summary().pairing, None);
        s.apply(&Event::Pairing { code });
        s.apply(&Event::Status { state: ConnState::Disconnected, text: String::new() });
        assert_eq!(s.summary().pairing, None);
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
        let line = Line { buffer: String::new(), kind: crate::chat::event::LineKind::Info, nick: String::new(), text: "x".into(), time: 0, highlight: false, own: false, msgid: String::new(), reply_to: String::new(), media: None };
        s.apply(&Event::Line { line });
        let sum = s.summary();
        assert_eq!(sum.queries, vec!["Bob"]);
        assert_eq!(sum.channels, vec!["#b"]);
    }
}

#[cfg(test)]
mod v02_tests {
    use super::*;

    fn entry(name: &str, users: u32, topic: &str) -> ChannelEntry {
        ChannelEntry { name: name.into(), users, topic: topic.into() }
    }

    #[test]
    fn unread_counters_go_up_and_reset() {
        let mut s = ServerState::new(10);
        s.apply(&Event::Joined { buffer: "#a".into() });
        s.bump("#A", false);
        s.bump("#a", true);
        s.bump("Bob", true);
        let sum = s.summary();
        let a = sum.activity.iter().find(|x| x.name == "#a").unwrap();
        assert_eq!((a.unread, a.mentions), (2, 1));
        assert_eq!(sum.activity.len(), 2);
        s.apply(&Event::Read { buffer: "#A".into() });
        assert_eq!(s.summary().activity.len(), 1);
        s.apply(&Event::Read { buffer: "inconnu".into() });
    }

    #[test]
    fn channel_list_pages_filter_and_sort() {
        let mut s = ServerState::new(10);
        s.apply(&Event::ListStart);
        assert!(s.list_page("", 0, false, 0, 10).loading);
        s.apply(&Event::ChannelList { entries: vec![entry("#rust", 500, "Le langage"), entry("#python", 900, "Un serpent"), entry("#vide", 1, "rust aussi"), entry("#A", 900, "")] });
        let p = s.list_page("", 0, false, 0, 10);
        assert!(!p.loading);
        assert_eq!((p.total, p.matched), (4, 4));
        assert_eq!(p.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["#A", "#python", "#rust", "#vide"], "by members, then name");
        let p = s.list_page("RUST", 0, false, 0, 10);
        assert_eq!(p.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["#rust", "#vide"], "name or topic, any case");
        assert_eq!(s.list_page("", 100, false, 0, 10).matched, 3);
        assert_eq!(s.list_page("", 0, true, 0, 2).entries[0].name, "#A");
        assert_eq!(s.list_page("", 0, false, 3, 10).entries.len(), 1);
        assert_eq!(s.list_page("", 0, false, 0, 100_000).entries.len(), 4, "page size is bounded");
    }

    #[test]
    fn whois_cards_are_kept_by_nick() {
        let mut s = ServerState::new(10);
        s.apply(&Event::Whois { info: Box::new(WhoisInfo { nick: "Bob".into(), realname: "B".into(), ..Default::default() }) });
        assert_eq!(s.whois("bob").unwrap().realname, "B");
        assert!(s.whois("alice").is_none());
    }
}
