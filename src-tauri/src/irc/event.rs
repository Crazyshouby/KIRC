//! What the protocol layer tells the rest of the application. These types are
//! serialised as they are to the pages (see ui/kirc.js).

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnState {
    Connecting,
    Registering,
    Connected,
    Disconnected,
    Reconnecting,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Msg,
    Action,
    Notice,
    Join,
    Part,
    Quit,
    Kick,
    Nick,
    Mode,
    Topic,
    Info,
    Error,
    Motd,
    Raw,
}

/// One line of a buffer. `buffer` is "" for the server console, otherwise a
/// channel name or the nick of a private conversation.
#[derive(Serialize, Clone, Debug)]
pub struct Line {
    pub buffer: String,
    pub kind: LineKind,
    pub nick: String,
    pub text: String,
    /// Milliseconds since the Unix epoch.
    pub time: i64,
    /// The line mentions us, or is a private message: worth a notification.
    pub highlight: bool,
    /// Typed by us.
    pub own: bool,
}

/// A member of a channel. `modes` holds the status symbols, highest first
/// ("@+" = operator and voiced).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct UserEntry {
    pub nick: String,
    pub modes: String,
    pub away: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Status { state: ConnState, text: String },
    Line { line: Line },
    /// The complete member list of a channel (after NAMES, or emptied on disconnect).
    Users { buffer: String, users: Vec<UserEntry> },
    UserSet { buffer: String, user: UserEntry },
    UserDel { buffer: String, nick: String },
    Topic { buffer: String, topic: String },
    /// We are on this channel: its window opens.
    Joined { buffer: String },
    /// We left this channel (or were kicked): its window closes.
    Parted { buffer: String },
    /// A private conversation exists: its window opens.
    Query { buffer: String },
    /// Our own nick and the network name as the server announces it.
    Me { nick: String, network: String },
    /// We are marked away (or not any more).
    Away { away: bool },
}

/// What the session asks the network layer to do.
#[derive(Clone, Debug)]
pub enum Out {
    /// Goes through the flood limiter.
    Send(String),
    /// Immediate (answers to PING, registration, QUIT).
    SendNow(String),
    Ui(Event),
    /// The user asked to leave: close after flushing, do not reconnect.
    Quit,
}
