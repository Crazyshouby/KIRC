//! The IRC client: wire format, protocol state machine, network layer.

pub use crate::chat::event;
pub mod message;
pub mod net;
pub mod session;

use crate::chat::{Start, Task};
use crate::secrets;
use net::ConnectionConfig;
use session::{Session, SessionConfig};

/// Prepares one IRC server of the address book: its session (nick, authentication, channels) and its connection.
pub fn launch(start: Start) -> Task {
    let Start { id, entry, settings, sink, commands } = start;
    let pick = |own: &str, default: &str| if own.trim().is_empty() { default.to_string() } else { own.trim().to_string() };
    let nick = pick(&entry.nick, &settings.nick);
    let alt = pick(&entry.alt_nick, &settings.alt_nick);
    let auth_secret = secrets::get(&id, "auth");
    let (sasl, nickserv_password) = match (entry.auth.as_str(), auth_secret) {
        ("sasl", Some(p)) => (Some((pick(&entry.account, &nick), p)), None),
        ("nickserv", Some(p)) => (None, Some(p)),
        _ => (None, None),
    };
    let cfg = SessionConfig {
        nick: nick.clone(),
        alt_nicks: if alt.is_empty() { Vec::new() } else { vec![alt] },
        user: pick(&entry.username, &settings.username),
        realname: pick(&entry.realname, &settings.realname),
        password: secrets::get(&id, "pass"),
        sasl,
        nickserv_password,
        autojoin: entry.autojoin_list(),
        quit_message: settings.quit_message.clone(),
        part_message: settings.part_message.clone(),
        version: format!("KIRC {}", env!("CARGO_PKG_VERSION")),
        ignores: settings.ignores.clone(),
        alert_words: settings.alert_words.clone(),
    };
    let conn = ConnectionConfig {
        host: entry.host.clone(),
        port: entry.port,
        tls: entry.tls,
        accept_invalid_certs: entry.accept_invalid_certs,
        autoreconnect: true,
    };
    Box::pin(async move { net::run(conn, Session::new(cfg), sink, commands).await })
}
