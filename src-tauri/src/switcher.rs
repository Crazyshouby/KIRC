//! What the "Aller à…" window (Ctrl+K) can jump to: the console of each server, every channel we are
//! on and every private conversation, with their unread counters and how recently they were looked at.
//! Ranking by what is typed is done by the page; this only lists.

use crate::chat::fold;
use crate::store::ServerSummary;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct SwitchItem {
    /// "chat" (a channel or a conversation) or "console".
    pub kind: &'static str,
    pub server: String,
    pub server_name: String,
    /// Channel or nick; empty for a console.
    pub buffer: String,
    pub unread: u32,
    pub mentions: u32,
    /// 0 = looked at most recently; None = never in this session.
    pub recent: Option<usize>,
    /// The server is connected: only then can a channel be joined or a person written to from here.
    pub connected: bool,
}

/// `servers`: (id, name) of the address book, in its order. `recent`: (server id, buffer), newest first.
pub fn build(servers: &[(String, String)], states: &HashMap<String, ServerSummary>, recent: &[(String, String)]) -> Vec<SwitchItem> {
    let rank = |server: &str, buffer: &str| recent.iter().position(|(s, b)| s == server && fold(b) == fold(buffer));
    let mut items = Vec::new();
    for (id, name) in servers {
        let Some(st) = states.get(id) else { continue };
        let connected = st.status == crate::chat::event::ConnState::Connected;
        items.push(SwitchItem { kind: "console", server: id.clone(), server_name: name.clone(), buffer: String::new(), unread: 0, mentions: 0, recent: rank(id, ""), connected });
        for b in st.channels.iter().chain(st.queries.iter()) {
            let act = st.activity.iter().find(|a| fold(&a.name) == fold(b));
            items.push(SwitchItem {
                kind: "chat",
                server: id.clone(),
                server_name: name.clone(),
                buffer: b.clone(),
                unread: act.map_or(0, |a| a.unread),
                mentions: act.map_or(0, |a| a.mentions),
                recent: rank(id, b),
                connected,
            });
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::event::ConnState;
    use crate::store::Activity;

    fn state(status: ConnState, channels: &[&str], queries: &[&str], activity: Vec<Activity>) -> ServerSummary {
        ServerSummary {
            status,
            status_text: String::new(),
            nick: "kirc".into(),
            network: String::new(),
            away: false,
            pairing: None,
            call: None,
            channels: channels.iter().map(|s| s.to_string()).collect(),
            activity,
            queries: queries.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn lists_consoles_channels_and_conversations_with_their_counters() {
        let servers = vec![("s1".to_string(), "Libera".to_string()), ("s2".to_string(), "OFTC".to_string()), ("s3".to_string(), "Jamais".to_string())];
        let mut states = HashMap::new();
        states.insert("s1".to_string(), state(ConnState::Connected, &["#rust", "#linux"], &["Bob"], vec![Activity { name: "#rust".into(), unread: 4, mentions: 1 }]));
        states.insert("s2".to_string(), state(ConnState::Disconnected, &[], &[], vec![]));
        let recent = vec![("s1".to_string(), "bob".to_string()), ("s1".to_string(), "#rust".to_string())];
        let items = build(&servers, &states, &recent);
        // s3 has no state yet (never connected): it has nothing to jump to.
        assert_eq!(items.iter().map(|i| (i.kind, i.server.as_str(), i.buffer.as_str())).collect::<Vec<_>>(), vec![
            ("console", "s1", ""), ("chat", "s1", "#rust"), ("chat", "s1", "#linux"), ("chat", "s1", "Bob"), ("console", "s2", ""),
        ]);
        let rust = items.iter().find(|i| i.buffer == "#rust").unwrap();
        assert_eq!((rust.unread, rust.mentions, rust.recent, rust.connected), (4, 1, Some(1), true));
        let bob = items.iter().find(|i| i.buffer == "Bob").unwrap();
        assert_eq!(bob.recent, Some(0), "recent is matched whatever the case");
        assert!(!items.last().unwrap().connected);
        assert_eq!(items.iter().find(|i| i.buffer == "#linux").unwrap().recent, None);
    }

    #[test]
    fn nothing_without_servers() {
        assert!(build(&[], &HashMap::new(), &[]).is_empty());
    }
}
