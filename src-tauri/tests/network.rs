//! The network layer against a scripted server on the loopback interface.

use kirc_lib::irc::event::{ConnState, Event, LineKind};
use kirc_lib::irc::net::{self, Command, ConnectionConfig, Sink};
use kirc_lib::irc::session::{Session, SessionConfig};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::timeout;

#[derive(Default)]
struct Collector(Mutex<Vec<Event>>);

impl Sink for Collector {
    fn emit(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}

impl Collector {
    async fn wait_for(&self, what: &str, f: impl Fn(&Event) -> bool) {
        let res = timeout(Duration::from_secs(10), async {
            loop {
                if self.0.lock().unwrap().iter().any(&f) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        if res.is_err() {
            panic!("never saw: {what}\nevents: {:#?}", self.0.lock().unwrap());
        }
    }
}

fn session() -> Session {
    Session::new(SessionConfig {
        nick: "kirc".into(),
        user: "kirc".into(),
        realname: "Test".into(),
        autojoin: vec!["#test".into()],
        quit_message: "Au revoir".into(),
        version: "KIRC test".into(),
        ..Default::default()
    })
}

fn conn(port: u16, autoreconnect: bool) -> ConnectionConfig {
    ConnectionConfig { host: "127.0.0.1".into(), port, tls: false, accept_invalid_certs: false, autoreconnect }
}

/// Reads lines from the client until one satisfies `pred`; returns all lines seen.
async fn read_until(rd: &mut BufReader<tokio::net::tcp::OwnedReadHalf>, pred: impl Fn(&str) -> bool) -> Vec<String> {
    let mut seen = Vec::new();
    timeout(Duration::from_secs(10), async {
        loop {
            let mut l = String::new();
            let n = rd.read_line(&mut l).await.expect("read");
            assert!(n > 0, "client closed early; saw {seen:?}");
            let l = l.trim_end().to_string();
            let hit = pred(&l);
            seen.push(l);
            if hit {
                return;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timeout; saw {seen:?}"));
    seen
}

async fn welcome(stream: TcpStream) -> (BufReader<tokio::net::tcp::OwnedReadHalf>, tokio::net::tcp::OwnedWriteHalf) {
    let (rd, mut wr) = stream.into_split();
    let mut rd = BufReader::new(rd);
    let lines = read_until(&mut rd, |l| l.starts_with("USER ")).await;
    assert_eq!(lines[0], "CAP LS 302");
    assert_eq!(lines[1], "NICK kirc");
    wr.write_all(b":srv CAP * LS :multi-prefix\r\n").await.unwrap();
    read_until(&mut rd, |l| l == "CAP REQ :multi-prefix").await;
    wr.write_all(b":srv CAP kirc ACK :multi-prefix\r\n").await.unwrap();
    read_until(&mut rd, |l| l == "CAP END").await;
    wr.write_all(b":srv 001 kirc :Welcome to the test network\r\n:srv 005 kirc PREFIX=(ov)@+ CHANTYPES=# NETWORK=TestNet :are supported\r\n").await.unwrap();
    (rd, wr)
}

#[tokio::test]
async fn full_session_over_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sink = Arc::new(Collector::default());
    let (tx, rx) = mpsc::unbounded_channel();
    let client = tokio::spawn(net::run(conn(port, false), session(), sink.clone(), rx));

    let (stream, _) = listener.accept().await.unwrap();
    let (mut rd, mut wr) = welcome(stream).await;

    // Autojoin after the welcome.
    read_until(&mut rd, |l| l == "JOIN #test").await;
    wr.write_all(b":kirc!k@h JOIN #test\r\n:srv 332 kirc #test :Sujet de test\r\n:srv 353 kirc = #test :@kirc +alice bob\r\n:srv 366 kirc #test :End\r\n").await.unwrap();
    sink.wait_for("Joined", |e| matches!(e, Event::Joined { buffer } if buffer == "#test")).await;
    sink.wait_for("Users", |e| matches!(e, Event::Users { users, .. } if users.len() == 3)).await;
    sink.wait_for("Topic", |e| matches!(e, Event::Topic { topic, .. } if topic == "Sujet de test")).await;
    sink.wait_for("network name", |e| matches!(e, Event::Me { network, .. } if network == "TestNet")).await;

    // Typed text goes out, incoming text comes in.
    tx.send(Command::Input { buffer: "#test".into(), text: "bonjour tout le monde".into() }).unwrap();
    read_until(&mut rd, |l| l == "PRIVMSG #test :bonjour tout le monde").await;
    wr.write_all(b":alice!a@h PRIVMSG #test :salut kirc\r\n").await.unwrap();
    sink.wait_for("highlighted message", |e| {
        matches!(e, Event::Line { line } if line.kind == LineKind::Msg && line.nick == "alice" && line.highlight)
    })
    .await;

    // PING is answered.
    wr.write_all(b"PING :abc123\r\n").await.unwrap();
    read_until(&mut rd, |l| l == "PONG abc123").await;

    // Leaving sends QUIT and ends the task.
    tx.send(Command::Disconnect("à plus".into())).unwrap();
    read_until(&mut rd, |l| l == "QUIT :à plus").await;
    timeout(Duration::from_secs(5), client).await.expect("client ends").unwrap();
    sink.wait_for("Disconnected", |e| matches!(e, Event::Status { state: ConnState::Disconnected, .. })).await;
}

#[tokio::test]
async fn oversized_line_is_skipped_and_the_link_survives() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sink = Arc::new(Collector::default());
    let (tx, rx) = mpsc::unbounded_channel();
    let client = tokio::spawn(net::run(conn(port, false), session(), sink.clone(), rx));

    let (stream, _) = listener.accept().await.unwrap();
    let (mut rd, mut wr) = welcome(stream).await;
    read_until(&mut rd, |l| l == "JOIN #test").await;

    let mut huge = b":srv NOTICE * :".to_vec();
    huge.extend(std::iter::repeat(b'x').take(200_000));
    huge.extend_from_slice(b"\r\nPING :still-alive\r\n");
    wr.write_all(&huge).await.unwrap();
    read_until(&mut rd, |l| l == "PONG still-alive").await;
    assert!(!sink.0.lock().unwrap().iter().any(|e| matches!(e, Event::Line { line } if line.text.len() > 10_000)));

    tx.send(Command::Disconnect(String::new())).unwrap();
    timeout(Duration::from_secs(5), client).await.expect("client ends").unwrap();
}

#[tokio::test]
async fn reconnects_and_rejoins_after_the_server_drops_the_link() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sink = Arc::new(Collector::default());
    let (tx, rx) = mpsc::unbounded_channel();
    let client = tokio::spawn(net::run(conn(port, true), session(), sink.clone(), rx));

    let (stream, _) = listener.accept().await.unwrap();
    let (mut rd, mut wr) = welcome(stream).await;
    read_until(&mut rd, |l| l == "JOIN #test").await;
    wr.write_all(b":kirc!k@h JOIN #test\r\n:srv 353 kirc = #test :kirc\r\n:srv 366 kirc #test :End\r\n").await.unwrap();
    sink.wait_for("Joined", |e| matches!(e, Event::Joined { .. })).await;
    drop(wr);
    drop(rd);

    sink.wait_for("Reconnecting", |e| matches!(e, Event::Status { state: ConnState::Reconnecting, text } if text.contains("nouvelle tentative"))).await;
    tx.send(Command::ConnectNow).unwrap();
    let (stream, _) = timeout(Duration::from_secs(10), listener.accept()).await.expect("second connection").unwrap();
    let (mut rd, _wr) = welcome(stream).await;
    read_until(&mut rd, |l| l == "JOIN #test").await;

    tx.send(Command::Disconnect(String::new())).unwrap();
    timeout(Duration::from_secs(5), client).await.expect("client ends").unwrap();
}

#[tokio::test]
async fn connection_refused_is_reported_without_reconnect() {
    // Bind then drop to get a port nobody listens on.
    let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let sink = Arc::new(Collector::default());
    let (_tx, rx) = mpsc::unbounded_channel();
    timeout(Duration::from_secs(10), net::run(conn(port, false), session(), sink.clone(), rx)).await.expect("run ends");
    sink.wait_for("failure", |e| matches!(e, Event::Status { state: ConnState::Disconnected, text } if text.contains("connexion impossible"))).await;
}
