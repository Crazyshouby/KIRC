//! What every messaging protocol has in common: the events a connection reports, the commands it takes, and the
//! list of what it can do. The rest of the application (windows, state, logs, notifications) only knows this
//! module; a protocol lives in a module of its own (`irc`) and is started by [`launch`].
//!
//! To add a protocol: a module that, given a [`Start`], returns the [`Task`] running one account (it reads its
//! [`Command`]s, reports [`event::Event`]s to the [`Sink`]), a variant of [`Protocol`] with its [`Caps`], and
//! one line in [`launch`].

pub mod event;

use crate::config::{ServerEntry, Settings};
use event::Event;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedReceiver;

/// Where the events of one account go (the application emits them to the pages).
pub trait Sink: Send + Sync + 'static {
    fn emit(&self, event: Event);
}

/// What the application asks of a connection.
#[derive(Debug)]
pub enum Command {
    /// A line typed in a buffer ("" = console).
    Input { buffer: String, text: String, reply: Option<(String, String)> },
    /// Leave, with this message; no reconnection.
    Disconnect(String),
    /// Skip the wait before the next reconnection attempt.
    ConnectNow,
    /// The ignore and alert lists changed.
    SetFilters { ignores: Vec<String>, alert_words: Vec<String> },
}

/// The protocols KIRC speaks. The id is what the address book stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Irc,
}

/// What a protocol can do; the pages hide what it cannot (a menu entry, a button).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Status symbols in the member list: give or take voice and operator rights.
    pub ranks: bool,
    /// Information card about a member.
    pub whois: bool,
    /// Kick and ban members.
    pub moderation: bool,
    /// The list of the server's channels.
    pub channel_list: bool,
    /// A channel has a topic that members can change.
    pub topic: bool,
}

impl Protocol {
    pub const ALL: [Protocol; 1] = [Protocol::Irc];

    pub fn id(self) -> &'static str {
        match self {
            Protocol::Irc => "irc",
        }
    }

    /// The protocol of an address-book entry; an entry saved before protocols existed has none, and is IRC.
    pub fn parse(id: &str) -> Option<Protocol> {
        let id = id.trim();
        if id.is_empty() {
            return Some(Protocol::Irc);
        }
        Protocol::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn caps(self) -> Caps {
        match self {
            Protocol::Irc => Caps { ranks: true, whois: true, moderation: true, channel_list: true, topic: true },
        }
    }
}

/// Everything a protocol needs to run one account.
pub struct Start {
    pub id: String,
    pub entry: ServerEntry,
    pub settings: Settings,
    pub sink: Arc<dyn Sink>,
    pub commands: UnboundedReceiver<Command>,
}

/// One account running: it ends when the user leaves (or, without reconnection, when the link drops).
pub type Task = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Prepares the account of `start` (reads its credentials) and returns what runs it. Nothing runs yet.
pub fn launch(start: Start) -> Result<Task, String> {
    match Protocol::parse(&start.entry.protocol) {
        Some(Protocol::Irc) => Ok(crate::irc::launch(start)),
        None => Err(format!("Protocole inconnu : {}", start.entry.protocol)),
    }
}

/// RFC 1459 case folding, the key under which a buffer is known: `[]\~` are the capitals of `{}|^`.
/// Every protocol uses it for now; one with case-sensitive names would need its own key.
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

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::{fold, Protocol};

    #[test]
    fn an_entry_without_a_protocol_is_irc() {
        assert_eq!(Protocol::parse(""), Some(Protocol::Irc));
        assert_eq!(Protocol::parse(" irc "), Some(Protocol::Irc));
        assert_eq!(Protocol::parse("irc"), Some(Protocol::Irc));
    }

    #[test]
    fn an_unknown_protocol_is_refused() {
        assert_eq!(Protocol::parse("telegram"), None);
        assert_eq!(Protocol::parse("IRC"), None);
    }

    #[test]
    fn every_protocol_has_a_distinct_id_that_parses_back() {
        for p in Protocol::ALL {
            assert_eq!(Protocol::parse(p.id()), Some(p));
        }
    }

    #[test]
    fn irc_can_do_everything_the_pages_offer() {
        let c = Protocol::Irc.caps();
        assert!(c.ranks && c.whois && c.moderation && c.channel_list && c.topic);
    }

    #[test]
    fn fold_follows_rfc1459() {
        assert_eq!(fold("[Nick]\\~"), "{nick}|^");
    }
}
