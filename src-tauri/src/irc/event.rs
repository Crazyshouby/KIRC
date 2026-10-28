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

/// What /whois learned about someone, shown as one card.
#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WhoisInfo {
    pub nick: String,
    pub user: String,
    pub host: String,
    pub realname: String,
    pub server: String,
    pub server_info: String,
    /// Seconds without activity.
    pub idle: Option<u64>,
    /// Connection time, seconds since the epoch.
    pub signon: Option<i64>,
    pub channels: String,
    pub account: String,
    pub away: String,
    pub secure: bool,
    pub operator: bool,
}

/// One row of the server's channel list.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ChannelEntry {
    pub name: String,
    pub users: u32,
    pub topic: String,
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
    /// The answer to /whois.
    Whois { info: Box<WhoisInfo> },
    /// A channel list is being received.
    ListStart,
    /// The whole list, as received. The application keeps it and tells the pages with `ListReady`.
    ChannelList { entries: Vec<ChannelEntry> },
    ListReady { count: usize },
    /// The window of this buffer was looked at: its unread counters go back to zero.
    Read { buffer: String },
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
