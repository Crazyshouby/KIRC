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
use std::path::PathBuf;
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
    /// WhatsApp, as a linked device (unofficial client: the account may be suspended). Built with the `whatsapp` feature.
    WhatsApp,
}

/// A protocol the pages can offer.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ProtocolInfo {
    pub id: &'static str,
    pub name: &'static str,
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
    pub const ALL: [Protocol; 2] = [Protocol::Irc, Protocol::WhatsApp];

    pub fn id(self) -> &'static str {
        match self {
            Protocol::Irc => "irc",
            Protocol::WhatsApp => "whatsapp",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Protocol::Irc => "IRC",
            Protocol::WhatsApp => "WhatsApp",
        }
    }

    /// Is this protocol part of this build?
    pub fn compiled(self) -> bool {
        match self {
            Protocol::Irc => true,
            Protocol::WhatsApp => cfg!(feature = "whatsapp"),
        }
    }

    /// The protocols of this build, for the address book.
    pub fn available() -> Vec<ProtocolInfo> {
        Protocol::ALL.into_iter().filter(|p| p.compiled()).map(|p| ProtocolInfo { id: p.id(), name: p.name() }).collect()
    }

    /// Does an account of this protocol have an address (host, port, nick, channels)?
    pub fn needs_host(self) -> bool {
        matches!(self, Protocol::Irc)
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
            Protocol::WhatsApp => Caps { ranks: false, whois: false, moderation: false, channel_list: false, topic: false },
        }
    }
}

/// Everything a protocol needs to run one account.
pub struct Start {
    /// The application's data directory (a protocol keeps its session files under it).
    pub dir: PathBuf,
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
        Some(Protocol::WhatsApp) => crate::whatsapp::launch(start),
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
    fn only_irc_has_an_address() {
        assert!(Protocol::Irc.needs_host());
        assert!(!Protocol::WhatsApp.needs_host());
        assert_eq!(Protocol::parse("whatsapp"), Some(Protocol::WhatsApp));
    }

    #[test]
    fn whatsapp_offers_none_of_the_irc_only_actions() {
        let c = Protocol::WhatsApp.caps();
        assert!(!c.ranks && !c.whois && !c.moderation && !c.channel_list && !c.topic);
    }

    #[test]
    fn the_address_book_offers_what_this_build_has() {
        let ids: Vec<&str> = Protocol::available().iter().map(|p| p.id).collect();
        assert!(ids.contains(&"irc"));
        assert_eq!(ids.contains(&"whatsapp"), cfg!(feature = "whatsapp"));
    }

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
        let mut seen = std::collections::HashSet::new();
        for p in Protocol::ALL {
            assert_eq!(Protocol::parse(p.id()), Some(p));
            assert!(seen.insert(p.id()), "{} twice", p.id());
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
