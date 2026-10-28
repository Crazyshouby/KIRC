//! The network side of one server: connect (TCP, TLS), feed the lines to the
//! [`Session`], write what it answers (with a flood limiter), keep the link
//! alive and reconnect with a growing delay.

use super::event::{ConnState, Event, Line, LineKind, Out};
use super::message::{Message, MAX_LINE};
use super::session::Session;
use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::time::{interval, sleep, timeout, MissedTickBehavior};

/// Where the events of one server go (the application emits them to the pages).
pub trait Sink: Send + Sync + 'static {
    fn emit(&self, event: Event);
}

#[derive(Clone, Debug)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    /// Accept a certificate that does not validate (self-signed servers).
    pub accept_invalid_certs: bool,
    pub autoreconnect: bool,
}

#[derive(Debug)]
pub enum Command {
    /// A line typed in a buffer ("" = console).
    Input { buffer: String, text: String },
    /// Leave: QUIT with this message, no reconnection.
    Disconnect(String),
    /// Skip the wait before the next reconnection attempt.
    ConnectNow,
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

enum End {
    /// The user left (QUIT or Disconnect).
    Quit,
    Lost(String),
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(90);
const PING_AFTER: Duration = Duration::from_secs(120);
const DEAD_AFTER: Duration = Duration::from_secs(240);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_QUEUE: usize = 400;

/// Runs one server until the user leaves (or, without auto-reconnect, until the link drops).
pub async fn run(conn: ConnectionConfig, mut session: Session, sink: Arc<dyn Sink>, mut cmds: UnboundedReceiver<Command>) {
    let mut attempt: u32 = 0;
    loop {
        let state = if attempt == 0 { ConnState::Connecting } else { ConnState::Reconnecting };
        let text = format!("Connexion à {}:{}…", conn.host, conn.port);
        sink.emit(Event::Status { state, text });

        let end = match connect(&conn).await {
            Ok(stream) => {
                attempt = 0;
                if !conn.tls {
                    sink.emit(line_info("Connexion non chiffrée : les messages et mots de passe transitent en clair"));
                }
                sink.emit(Event::Status { state: ConnState::Registering, text: "Enregistrement…".into() });
                let (end, fresh_cmds) = link(stream, &mut session, &*sink, cmds).await;
                cmds = fresh_cmds;
                end
            }
            Err(e) => End::Lost(e),
        };

        let reason = match &end {
            End::Quit => "Déconnecté".to_string(),
            End::Lost(r) => r.clone(),
        };
        for out in session.on_disconnect(&reason, now_ms()) {
            if let Out::Ui(ev) = out {
                sink.emit(ev);
            }
        }
        if matches!(end, End::Quit) || session.is_quitting() || !conn.autoreconnect {
            sink.emit(Event::Status { state: ConnState::Disconnected, text: reason });
            return;
        }

        // Wait before trying again: 5, 10, 20, 40, 60, 60... seconds.
        attempt += 1;
        let wait = (5u64 << (attempt - 1).min(4)).min(60);
        sink.emit(Event::Status {
            state: ConnState::Reconnecting,
            text: format!("{reason} — nouvelle tentative dans {wait} s"),
        });
        let deadline = sleep(Duration::from_secs(wait));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                cmd = cmds.recv() => match cmd {
                    None => return,
                    Some(Command::Disconnect(_)) => {
                        sink.emit(Event::Status { state: ConnState::Disconnected, text: "Déconnecté".into() });
                        return;
                    }
                    Some(Command::ConnectNow) => break,
                    Some(Command::Input { buffer, .. }) => {
                        sink.emit(line_error(&buffer, "Non connecté : le message n'a pas été envoyé"));
                    }
                },
            }
        }
    }
}

fn line_info(text: &str) -> Event {
    Event::Line {
        line: Line { buffer: String::new(), kind: LineKind::Info, nick: String::new(), text: text.into(), time: now_ms(), highlight: false, own: false },
    }
}

fn line_error(buffer: &str, text: &str) -> Event {
    Event::Line {
        line: Line { buffer: buffer.into(), kind: LineKind::Error, nick: String::new(), text: text.into(), time: now_ms(), highlight: false, own: false },
    }
}

async fn connect(conn: &ConnectionConfig) -> Result<Box<dyn Io>, String> {
    let tcp = timeout(CONNECT_TIMEOUT, TcpStream::connect((conn.host.as_str(), conn.port)))
        .await
        .map_err(|_| "délai de connexion dépassé".to_string())?
        .map_err(|e| format!("connexion impossible : {e}"))?;
    let _ = tcp.set_nodelay(true);
    if !conn.tls {
        return Ok(Box::new(tcp));
    }
    let mut b = native_tls::TlsConnector::builder();
    b.min_protocol_version(Some(native_tls::Protocol::Tlsv12));
    if conn.accept_invalid_certs {
        b.danger_accept_invalid_certs(true).danger_accept_invalid_hostnames(true);
    }
    let connector = tokio_native_tls::TlsConnector::from(b.build().map_err(|e| format!("TLS indisponible : {e}"))?);
    let tls = timeout(CONNECT_TIMEOUT, connector.connect(&conn.host, tcp))
        .await
        .map_err(|_| "délai de la négociation TLS dépassé".to_string())?
        .map_err(|e| format!("échec TLS : {e}"))?;
    Ok(Box::new(tls))
}

/// Reads one line (without its terminator) of at most `MAX_LINE` bytes. Longer
/// lines are skipped whole, so a hostile server cannot make us buffer without
/// bound. `Ok(None)` is the end of the stream.
async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    let mut overflow = false;
    loop {
        let (used, done) = {
            let chunk = r.fill_buf().await?;
            if chunk.is_empty() {
                return Ok(None);
            }
            match chunk.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    if buf.len() + i <= MAX_LINE {
                        buf.extend_from_slice(&chunk[..i]);
                    } else {
                        overflow = true;
                    }
                    (i + 1, true)
                }
                None => {
                    if buf.len() + chunk.len() <= MAX_LINE {
                        buf.extend_from_slice(chunk);
                    } else {
                        overflow = true;
                    }
                    (chunk.len(), false)
                }
            }
        };
        r.consume(used);
        if done {
            return Ok(Some(if overflow { Vec::new() } else { buf }));
        }
    }
}

/// Token bucket: bursts of up to 8 lines, then two per second.
struct Limiter {
    queue: VecDeque<String>,
    tokens: f64,
    last: Instant,
}

impl Limiter {
    fn new() -> Self {
        Limiter { queue: VecDeque::new(), tokens: 8.0, last: Instant::now() }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * 2.0).min(8.0);
        self.last = now;
    }

    fn take(&mut self) -> Option<String> {
        self.refill();
        if self.tokens >= 1.0 && !self.queue.is_empty() {
            self.tokens -= 1.0;
            return self.queue.pop_front();
        }
        None
    }
}

async fn write_line<W: AsyncWrite + Unpin>(w: &mut W, line: &str) -> Result<(), String> {
    let mut data = Vec::with_capacity(line.len() + 2);
    data.extend_from_slice(line.as_bytes());
    data.extend_from_slice(b"\r\n");
    match timeout(WRITE_TIMEOUT, async {
        w.write_all(&data).await?;
        w.flush().await
    })
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(format!("écriture impossible : {e}")),
        Err(_) => Err("écriture bloquée".to_string()),
    }
}

/// One connection, from the first line to the last. Gives the command channel back.
async fn link(
    stream: Box<dyn Io>,
    session: &mut Session,
    sink: &dyn Sink,
    mut cmds: UnboundedReceiver<Command>,
) -> (End, UnboundedReceiver<Command>) {
    let (rd, mut wr) = tokio::io::split(stream);
    let (line_tx, mut line_rx) = mpsc::channel::<io::Result<Option<Vec<u8>>>>(256);
    let reader = tokio::spawn(async move {
        let mut r = BufReader::new(rd);
        loop {
            let res = read_line(&mut r).await;
            let stop = !matches!(res, Ok(Some(_)));
            if line_tx.send(res).await.is_err() || stop {
                break;
            }
        }
    });

    let mut limiter = Limiter::new();
    let started = Instant::now();
    let mut last_rx = Instant::now();
    let mut pinged = false;
    let mut last_session_tick = Instant::now();
    let mut tick = interval(Duration::from_millis(200));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut quit_at: Option<Instant> = None;

    let end = 'main: {
        let first = session.start();
        if let Err(e) = dispatch(first, &mut wr, &mut limiter, sink, &mut quit_at).await {
            break 'main End::Lost(e);
        }
        loop {
            tokio::select! {
                got = line_rx.recv() => {
                    match got {
                        None | Some(Ok(None)) => break 'main if quit_at.is_some() { End::Quit } else { End::Lost("connexion fermée par le serveur".into()) },
                        Some(Err(e)) => break 'main End::Lost(format!("lecture impossible : {e}")),
                        Some(Ok(Some(bytes))) => {
                            last_rx = Instant::now();
                            pinged = false;
                            let text = String::from_utf8_lossy(&bytes);
                            if let Some(msg) = Message::parse(&text) {
                                let outs = session.handle(&msg, now_ms());
                                if let Err(e) = dispatch(outs, &mut wr, &mut limiter, sink, &mut quit_at).await {
                                    break 'main End::Lost(e);
                                }
                                if msg.command == "ERROR" && quit_at.is_some() {
                                    break 'main End::Quit;
                                }
                            }
                        }
                    }
                }
                cmd = cmds.recv() => {
                    match cmd {
                        None => break 'main End::Quit,
                        Some(Command::Disconnect(msg)) => {
                            let outs = session.user_input("", &format!("/quit {msg}"), now_ms());
                            let _ = dispatch(outs, &mut wr, &mut limiter, sink, &mut quit_at).await;
                            break 'main End::Quit;
                        }
                        Some(Command::ConnectNow) => {}
                        Some(Command::Input { buffer, text }) => {
                            let outs = session.user_input(&buffer, &text, now_ms());
                            if let Err(e) = dispatch(outs, &mut wr, &mut limiter, sink, &mut quit_at).await {
                                break 'main End::Lost(e);
                            }
                            if quit_at.is_some() {
                                // QUIT has been written: give the server a moment to close.
                                let _ = timeout(Duration::from_secs(2), async { while let Some(Ok(Some(_))) = line_rx.recv().await {} }).await;
                                break 'main End::Quit;
                            }
                        }
                    }
                }
                _ = tick.tick() => {
                    while let Some(line) = limiter.take() {
                        if let Err(e) = write_line(&mut wr, &line).await {
                            break 'main End::Lost(e);
                        }
                    }
                    if last_session_tick.elapsed() >= Duration::from_secs(1) {
                        last_session_tick = Instant::now();
                        let outs = session.tick(now_ms());
                        if let Err(e) = dispatch(outs, &mut wr, &mut limiter, sink, &mut quit_at).await {
                            break 'main End::Lost(e);
                        }
                    }
                    if !session.is_registered() && started.elapsed() > REGISTRATION_TIMEOUT {
                        break 'main End::Lost("délai d'enregistrement dépassé".into());
                    }
                    let idle = last_rx.elapsed();
                    if idle > DEAD_AFTER {
                        break 'main End::Lost("délai dépassé (le serveur ne répond plus)".into());
                    }
                    if idle > PING_AFTER && !pinged {
                        pinged = true;
                        if let Err(e) = write_line(&mut wr, "PING :kirc").await {
                            break 'main End::Lost(e);
                        }
                    }
                }
            }
        }
    };

    reader.abort();
    let _ = wr.shutdown().await;
    (end, cmds)
}

/// Applies what the session produced: immediate lines are written now, the
/// others wait for the flood limiter, events go to the application.
async fn dispatch<W: AsyncWrite + Unpin>(
    outs: Vec<Out>,
    wr: &mut W,
    limiter: &mut Limiter,
    sink: &dyn Sink,
    quit_at: &mut Option<Instant>,
) -> Result<(), String> {
    for out in outs {
        match out {
            Out::SendNow(l) => write_line(wr, &l).await?,
            Out::Send(l) => {
                if limiter.queue.len() >= MAX_QUEUE {
                    sink.emit(line_error("", "Trop de messages en attente : certains ont été abandonnés"));
                    limiter.queue.pop_front();
                }
                limiter.queue.push_back(l);
            }
            Out::Ui(ev) => sink.emit(ev),
            Out::Quit => *quit_at = Some(Instant::now()),
        }
    }
    // Send right away what the limiter allows (the usual case: one line).
    while let Some(line) = limiter.take() {
        write_line(wr, &line).await?;
    }
    Ok(())
}
