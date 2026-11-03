//! Playing on two computers in the same network. Not part of the original game.
//!
//! The port has no randomness and no clock, so the same inputs give the same game: only inputs go over the network.
//!
//! `NetMode::TakeTurns`: both computers run one game. The inputs of the player whose turn it is go out, `GameState::player` says
//! on both sides whose turn it is, and the other computer watches.
//!
//! `NetMode::SameTime`: each player plays their own game at the same time. Each computer also runs a copy of the other game with
//! the other player's inputs: it fills the status line, and it is what a player who is out watches.
//!
//! A checksum now and then proves that both computers still agree.

use crate::result::Winner;
use giana_core::state::{ControllerInput, GameState};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::{channel, sync_channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// TCP for the game, UDP for finding a game in the network.
pub const PORT: u16 = 47064;
const HELLO: &[u8; 8] = b"GIANANT5";
/// The host answers with this instead of the game mode when the other computer runs another build.
const REFUSED: u8 = 0xFF;

/// Which build this is: the version and a hash of the game logic. Computers with other builds would drift apart, so they
/// refuse each other in the handshake.
pub const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), "-", env!("GIANA_CORE_HASH"));

fn build_id() -> [u8; 4] {
    let mut h: u32 = 0x811C_9DC5;
    for b in BUILD.bytes() {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    h.to_le_bytes()
}
const SEARCH: &[u8] = b"GIANA-SEARCH";
const OFFER: &[u8] = b"GIANA-OFFER";
const CHECK_EVERY: u64 = 64;
/// Inputs of the other computer that may wait: one byte each, so this bounds the memory to about 1 MB. The other side can
/// legitimately be far ahead (a window that was hidden for a while), so the limit is far above any real game.
const MAX_QUEUED_INPUTS: usize = 1_000_000;
/// Chunks (up to 1 KB each) that may wait between the reader thread and the game: far more than a tick brings.
const READ_CHUNKS: usize = 64;

/// How the watched game plays the inputs of the other computer: a small buffer that grows after a hiccup.
///
/// Wireless networks deliver in bursts (a hold of 40-150 ms every second or two is common). Played as soon as they arrive,
/// the watched game stands still for a step and then jumps two: invisible while the player stands, a visible stutter while
/// the screen scrolls. A buffer of a few steps (40-160 ms, nothing a watcher notices) hides the bursts.
#[derive(Debug, Clone)]
pub struct Playout {
    /// steps that are kept in hand
    target: usize,
    /// waiting until `target` steps are in hand
    buffering: bool,
    /// ticks since the last input came
    idle: u32,
    /// ticks the buffer has been empty (to tell a hiccup from a pause)
    dry: u32,
}

impl Playout {
    const START: usize = 2;
    const MAX: usize = 8;
    /// No input for this long (200 ms): the other side stopped (turn over, pause, game over), play what is left.
    const IDLE_FLUSH: u32 = 10;
    /// An empty buffer that fills again within this time (500 ms) was a network hiccup: keep more in hand.
    const HICCUP: u32 = 25;

    pub fn new() -> Self {
        Self { target: Self::START, buffering: true, idle: 0, dry: 0 }
    }

    /// Steps the watched game takes in this tick. `queued` inputs are waiting, `arrived` of them came since the last tick.
    pub fn steps(&mut self, queued: usize, arrived: usize) -> usize {
        self.idle = if arrived > 0 { 0 } else { self.idle.saturating_add(1) };
        if queued == 0 {
            if !self.buffering {
                self.buffering = true;
                self.dry = 0;
            }
            self.dry = self.dry.saturating_add(1);
            return 0;
        }
        if self.buffering {
            if queued >= self.target || self.idle >= Self::IDLE_FLUSH {
                self.buffering = false;
                if self.dry > 0 && self.dry <= Self::HICCUP && queued >= self.target {
                    self.target = (self.target + 1).min(Self::MAX);
                }
                self.dry = 0;
            } else {
                return 0;
            }
        }
        // What is in hand after a burst is the buffer that the next burst needs: it is kept (at most about 0.2 s of delay,
        // nothing a watcher notices). Only a real backlog is caught up with a second step.
        if queued > Self::MAX + 4 { 2 } else { 1 }
    }
}

impl Default for Playout {
    fn default() -> Self {
        Self::new()
    }
}

/// The longest a write may hold up the game, and how long after a failed write the reader gets to deliver what is on its way.
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);
const WRITE_FAILED_GRACE: Duration = Duration::from_millis(500);

const MSG_INPUT: u8 = 1;
const MSG_CHECK: u8 = 2;
const MSG_BYE: u8 = 3;
/// followed by 1 when the sender's pause menu holds up the game, 0 when it goes on
const MSG_PAUSE: u8 = 4;

/// Why a connection could not be made. `text` is what the player reads.
#[derive(Debug)]
pub enum NetError {
    /// nobody listens at that address
    Refused,
    TimedOut,
    BadAddress,
    /// the other computer runs another build
    WrongVersion,
    Other(std::io::Error),
}

impl NetError {
    pub fn text(&self) -> &'static str {
        match self {
            NetError::Refused => "NOBODY IS HOSTING AT THAT ADDRESS",
            NetError::TimedOut => "NO ANSWER - CHECK THE ADDRESS",
            NetError::BadAddress => "THAT IS NOT A VALID ADDRESS",
            NetError::WrongVersion => "OTHER PLAYER HAS ANOTHER VERSION",
            NetError::Other(_) => "COULD NOT CONNECT",
        }
    }
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Other(e) => write!(f, "{}: {e}", self.text()),
            _ => f.write_str(self.text()),
        }
    }
}

impl std::error::Error for NetError {}

impl From<std::io::Error> for NetError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind::*;
        match e.kind() {
            ConnectionRefused => NetError::Refused,
            TimedOut | WouldBlock => NetError::TimedOut,
            _ => NetError::Other(e),
        }
    }
}

pub fn input_bits(i: &ControllerInput) -> u8 {
    i.left as u8 | (i.right as u8) << 1 | (i.down as u8) << 2 | (i.jump as u8) << 3 | (i.fire as u8) << 4 | (i.item as u8) << 5
}

pub fn input_from_bits(bits: u8) -> ControllerInput {
    let mut i = ControllerInput::new();
    i.left = bits & 1 != 0;
    i.right = bits & 2 != 0;
    i.down = bits & 4 != 0;
    i.jump = bits & 8 != 0;
    i.fire = bits & 16 != 0;
    i.item = bits & 32 != 0;
    i
}

/// FNV-1a over what shows when two games have drifted apart.
pub fn checksum(g: &GameState) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut add = |b: u8| h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    g.screen_ram.iter().for_each(|&b| add(b));
    g.color_ram.iter().for_each(|&b| add(b));
    g.score_bcd.iter().for_each(|&b| add(b));
    g.player_save.iter().for_each(|&b| add(b));
    g.en.col.iter().for_each(|&b| add(b));
    g.en.vic_frame.xy.iter().for_each(|&b| add(b));
    [g.player, g.lives_c64, g.scroll_fine, g.stage as u8].iter().for_each(|&b| add(b));
    h
}

/// How two computers play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetMode {
    /// one game, the original rule: the turn changes with every lost life
    TakeTurns,
    /// each player has their own game; who is out watches the other
    SameTime,
}

/// The rules byte after the mode byte: the enhancements that change how the game plays (`Enhancements::rules`), as the host
/// chose them. Both computers have to play by the same rules, so the host decides and the other computer takes them over.
/// No such enhancement exists yet, so this build knows no bit: a host that sends one is from a newer build and is refused.
pub const KNOWN_RULES: u8 = 0;

/// The byte after the handshake: bit 0 is the game mode, bit 1 the rule for the winner. Both computers show the same result.
fn mode_byte(m: NetMode, w: Winner) -> u8 {
    let mode = match m {
        NetMode::TakeTurns => 0,
        NetMode::SameTime => 1,
    };
    mode | w.code() << 1
}

fn mode_from(b: u8) -> Option<(NetMode, Winner)> {
    let mode = match b & 1 {
        0 => NetMode::TakeTurns,
        _ => NetMode::SameTime,
    };
    Some((mode, Winner::from_code(b >> 1)?))
}

/// The address other computers can reach. The UDP "connect" only picks the network card, nothing is sent.
pub fn local_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    s.connect(("192.0.2.1", 9)).ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

/// Waiting for the second player.
pub struct Host {
    listener: TcpListener,
    finder: Option<UdpSocket>,
    mode: NetMode,
    winner: Winner,
    rules: u8,
    /// Connections whose handshake runs on a thread of its own, so that a silent client cannot hold up the game loop
    greeting: Vec<Pending>,
}

impl Host {
    pub fn new(mode: NetMode) -> std::io::Result<Self> {
        Self::on_port(mode, PORT)
    }

    pub fn on_port(mode: NetMode, port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        listener.set_nonblocking(true)?;
        // Finding the game is a convenience: without it the address can still be typed in
        let finder = UdpSocket::bind(("0.0.0.0", port)).ok().filter(|s| s.set_nonblocking(true).is_ok());
        Ok(Self { listener, finder, mode, winner: Winner::default(), rules: 0, greeting: Vec::new() })
    }

    /// The rules of the enhancements that change the game, sent to the computer that joins (see `KNOWN_RULES`).
    pub fn set_rules(&mut self, rules: u8) {
        self.rules = rules;
    }

    /// The rule for the winner, sent to the computer that joins (the host decides, so that both show the same result).
    pub fn set_winner(&mut self, winner: Winner) {
        self.winner = winner;
    }

    pub fn port(&self) -> u16 {
        self.listener.local_addr().map_or(PORT, |a| a.port())
    }

    /// False when the UDP socket could not be opened: other computers cannot find this game, they have to type the address.
    pub fn can_be_found(&self) -> bool {
        self.finder.is_some()
    }

    /// Answer searches and take a player who connects. A client that fails the handshake is an `Err`.
    pub fn poll(&mut self) -> Option<Result<Session, NetError>> {
        if let Some(s) = &self.finder {
            let mut buf = [0u8; 32];
            while let Ok((n, from)) = s.recv_from(&mut buf) {
                if &buf[..n] == SEARCH {
                    let _ = s.send_to(OFFER, from);
                }
            }
        }
        loop {
            match self.listener.accept() {
                Ok((stream, from)) => {
                    // One handshake per address and a few in all, the newest wins: a flood of connections cannot start
                    // any number of threads, and silent connections cannot lock out a player who tries (again).
                    // A dropped handshake is shut down at once, which ends its thread.
                    let mut k = 0;
                    while k < self.greeting.len() {
                        if self.greeting[k].ip == from.ip() {
                            self.greeting.remove(k).cancel();
                        } else {
                            k += 1;
                        }
                    }
                    if self.greeting.len() >= MAX_GREETING {
                        self.greeting.remove(0).cancel();
                    }
                    // Threads that are still ending are counted as well
                    if LIVE_GREETERS.load(Ordering::Relaxed) >= MAX_GREETERS {
                        drop(stream);
                        continue;
                    }
                    let Ok(handle) = stream.try_clone() else { continue };
                    let (tx, rx) = channel();
                    let (mode, winner, rules) = (self.mode, self.winner, self.rules);
                    LIVE_GREETERS.fetch_add(1, Ordering::Relaxed);
                    let spawned = std::thread::Builder::new().name("net-greet".into()).spawn(move || {
                        // A session nobody takes any more is closed, so that its reader thread ends too
                        if let Err(unsent) = tx.send(greet(stream, mode, winner, rules)) {
                            if let Ok(mut s) = unsent.0 {
                                s.leave();
                            }
                        }
                        LIVE_GREETERS.fetch_sub(1, Ordering::Relaxed);
                    });
                    match spawned {
                        Ok(_) => self.greeting.push(Pending { ip: from.ip(), rx, stream: handle }),
                        Err(_) => {
                            LIVE_GREETERS.fetch_sub(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Some(Err(e.into())),
            }
        }
        let mut i = 0;
        while i < self.greeting.len() {
            match self.greeting[i].rx.try_recv() {
                Ok(result) => {
                    self.greeting.remove(i);
                    return Some(result);
                }
                Err(TryRecvError::Empty) => i += 1,
                Err(TryRecvError::Disconnected) => {
                    self.greeting.remove(i);
                }
            }
        }
        None
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        for p in self.greeting.drain(..) {
            p.cancel();
        }
    }
}

/// A handshake in progress.
struct Pending {
    ip: IpAddr,
    rx: Receiver<Result<Session, NetError>>,
    /// a handle to the same connection, to end the handshake from outside
    stream: TcpStream,
}

impl Pending {
    /// End the handshake: its read fails at once, and a session it may already have made is closed.
    fn cancel(self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        if let Ok(Ok(mut s)) = self.rx.try_recv() {
            s.leave();
        }
    }
}

/// Handshakes the host runs at the same time.
const MAX_GREETING: usize = 16;

/// Handshake threads that may run at once, including those whose handshake was already dropped.
const MAX_GREETERS: usize = 32;
static LIVE_GREETERS: AtomicUsize = AtomicUsize::new(0);

/// `read_exact` with one deadline for the whole buffer: a client that sends a byte now and then cannot stretch it.
fn read_exact_until(stream: &mut TcpStream, buf: &mut [u8], deadline: std::time::Instant) -> std::io::Result<()> {
    let mut done = 0;
    while done < buf.len() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(Some(left))?;
        match stream.read(&mut buf[done..]) {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => done += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn greet(mut stream: TcpStream, mode: NetMode, winner: Winner, rules: u8) -> Result<Session, NetError> {
    stream.set_nonblocking(false)?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut hello = [0u8; 8];
    read_exact_until(&mut stream, &mut hello, deadline)?;
    let mut id = [0u8; 4];
    // A program of an older version sends no build id: its hello is already wrong
    if &hello == HELLO {
        read_exact_until(&mut stream, &mut id, deadline)?;
    }
    if &hello != HELLO || id != build_id() {
        let _ = stream.write_all(&[REFUSED]);
        return Err(NetError::WrongVersion);
    }
    stream.write_all(&[mode_byte(mode, winner), rules])?;
    let mut session = Session::start(stream, 0, mode)?;
    session.winner = winner;
    session.rules = rules;
    Ok(session)
}

/// The address of a host. Without a port the usual one. Names are not looked up: that could hold up the game for a long time.
pub fn parse_address(addr: &str) -> Result<SocketAddr, NetError> {
    let addr = addr.trim();
    let parsed = addr.parse::<SocketAddr>().ok().or_else(|| addr.parse::<IpAddr>().ok().map(|ip| SocketAddr::new(ip, PORT)));
    parsed.filter(|a| a.port() != 0).ok_or(NetError::BadAddress)
}

/// Connect to a host and wait for the handshake. It can take several seconds: the game loop uses `Joiner`.
pub fn join(addr: &str) -> Result<Session, NetError> {
    let target = parse_address(addr)?;
    let mut stream = TcpStream::connect_timeout(&target, Duration::from_secs(3))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let mut hello = HELLO.to_vec();
    hello.extend_from_slice(&build_id());
    stream.write_all(&hello)?;
    let mut mode = [0u8; 1];
    stream.read_exact(&mut mode)?;
    let (mode, winner) = mode_from(mode[0]).ok_or(NetError::WrongVersion)?;
    let mut rules = [0u8; 1];
    stream.read_exact(&mut rules)?;
    if rules[0] & !KNOWN_RULES != 0 {
        return Err(NetError::WrongVersion);
    }
    let mut session = Session::start(stream, 1, mode)?;
    session.winner = winner;
    session.rules = rules[0];
    Ok(session)
}

/// A connection being made on a thread of its own. Dropping it cancels: a session that still arrives says goodbye at once.
pub struct Joiner {
    rx: Receiver<Result<Session, NetError>>,
}

impl Joiner {
    pub fn start(addr: &str) -> Self {
        let addr = addr.to_string();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            if let Err(unwanted) = tx.send(join(&addr)) {
                if let Ok(mut s) = unwanted.0 {
                    s.leave();
                }
            }
        });
        Self { rx }
    }

    pub fn poll(&mut self) -> Option<Result<Session, NetError>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(NetError::Other(std::io::Error::other("connection thread ended")))),
        }
    }
}

/// Looks for hosts in the network.
pub struct Finder {
    socket: UdpSocket,
    last: Option<Instant>,
    pub hosts: Vec<String>,
    /// the last search could not be sent, e.g. without a network
    send_failed: bool,
}

impl Finder {
    pub fn new() -> std::io::Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", 0))?;
        socket.set_broadcast(true)?;
        socket.set_nonblocking(true)?;
        Ok(Self { socket, last: None, hosts: Vec::new(), send_failed: false })
    }

    /// Searching does not work (no network): a typed address is the only way.
    pub fn broken(&self) -> bool {
        self.send_failed
    }

    pub fn poll(&mut self) {
        if self.last.map_or(true, |t| t.elapsed() > Duration::from_secs(1)) {
            self.last = Some(Instant::now());
            self.send_failed = self.socket.send_to(SEARCH, ("255.255.255.255", PORT)).is_err();
        }
        let mut buf = [0u8; 32];
        while let Ok((n, from)) = self.socket.recv_from(&mut buf) {
            let ip = from.ip().to_string();
            if &buf[..n] == OFFER && !self.hosts.contains(&ip) {
                self.hosts.push(ip);
            }
        }
    }
}

/// A running game with another computer.
pub struct Session {
    stream: TcpStream,
    /// chunks from the reader thread; the channel is bounded, so a peer that floods is slowed down by TCP itself
    incoming: Receiver<Vec<u8>>,
    bytes: VecDeque<u8>,
    inputs: VecDeque<u8>,
    /// inputs that came since the last tick
    arrived: usize,
    playout: Playout,
    /// the player this computer plays: 0 on the host (Giana), 1 on the other (Maria)
    pub me: u8,
    pub mode: NetMode,
    /// the rule for the winner, as the host chose it
    pub winner: Winner,
    /// the rules of the enhancements that change the game, as the host chose them (see `KNOWN_RULES`)
    pub rules: u8,
    tick: u64,
    /// steps of this player's own game (`NetMode::SameTime`)
    my_tick: u64,
    my_checks: VecDeque<(u64, u32)>,
    peer_checks: VecDeque<(u64, u32)>,
    pub ended: Option<&'static str>,
    /// checksums that agreed so far
    pub verified: u64,
    /// the other player said goodbye
    left: bool,
    /// when a write failed
    write_failed: Option<Instant>,
    /// what the other computer was told about our pause menu
    sent_paused: bool,
    peer_paused: bool,
    /// `leave` was called (it already closed the connection)
    left_on_purpose: bool,
}

impl Drop for Session {
    /// A session that is thrown away closes its connection, so the reader thread ends and the other side hears of it.
    fn drop(&mut self) {
        if !self.left_on_purpose {
            let _ = self.stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl Session {
    fn start(stream: TcpStream, me: u8, mode: NetMode) -> std::io::Result<Self> {
        stream.set_nodelay(true)?;
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        // At most READ_CHUNKS chunks wait: when the game does not take them, the reader blocks and TCP holds the peer back
        let (tx, incoming) = sync_channel::<Vec<u8>>(READ_CHUNKS);
        let mut reader = stream.try_clone()?;
        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            stream,
            incoming,
            bytes: VecDeque::new(),
            inputs: VecDeque::new(),
            arrived: 0,
            playout: Playout::new(),
            me,
            mode,
            winner: Winner::default(),
            rules: 0,
            tick: 0,
            my_tick: 0,
            my_checks: VecDeque::new(),
            peer_checks: VecDeque::new(),
            ended: None,
            verified: 0,
            left: false,
            write_failed: None,
            sent_paused: false,
            peer_paused: false,
            left_on_purpose: false,
        })
    }

    /// The game of this computer, and the copy of the other player's game when each has their own.
    pub fn new_games(&self) -> (GameState, Option<GameState>) {
        match self.mode {
            NetMode::TakeTurns => (GameState::new_port_game_two(), None),
            NetMode::SameTime => (GameState::new_port_game_as(self.me), Some(GameState::new_port_game_as(self.me ^ 1))),
        }
    }

    /// A failed write ends the connection, but not at once: what already arrived still counts, such as the goodbye of the other
    /// player. Nothing more is written, so a computer that hangs can only hold the game up once, for `WRITE_TIMEOUT`.
    fn send(&mut self, msg: &[u8]) {
        if self.write_failed.is_none() && self.stream.write_all(msg).is_err() {
            self.write_failed = Some(Instant::now());
        }
    }

    pub fn leave(&mut self) {
        self.left_on_purpose = true;
        self.send(&[MSG_BYE]);
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    fn receive(&mut self) {
        let mut closed = false;
        // at most what the bounded channel holds: a peer that floods cannot hold the game loop
        for _ in 0..READ_CHUNKS {
            match self.incoming.try_recv() {
                Ok(chunk) => self.bytes.extend(chunk),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        while let Some(&kind) = self.bytes.front() {
            let need = match kind {
                MSG_INPUT => 2,
                MSG_CHECK => 13,
                MSG_BYE => 1,
                MSG_PAUSE => 2,
                _ => {
                    self.ended = Some("CONNECTION LOST");
                    return;
                }
            };
            if self.bytes.len() < need {
                break;
            }
            let m: Vec<u8> = self.bytes.drain(..need).collect();
            match kind {
                MSG_INPUT => {
                    self.inputs.push_back(m[1]);
                    self.arrived += 1;
                    if self.inputs.len() > MAX_QUEUED_INPUTS {
                        self.ended = Some("CONNECTION LOST");
                        return;
                    }
                }
                MSG_CHECK => {
                    let tick = u64::from_le_bytes(m[1..9].try_into().unwrap());
                    let sum = u32::from_le_bytes(m[9..13].try_into().unwrap());
                    self.peer_checks.push_back((tick, sum));
                    if self.peer_checks.len() > MAX_QUEUED_INPUTS / CHECK_EVERY as usize + 8 {
                        self.ended = Some("CONNECTION LOST");
                        return;
                    }
                }
                MSG_PAUSE => self.peer_paused = m[1] != 0,
                _ => self.left = true,
            }
        }
        closed |= self.write_failed.map_or(false, |t| t.elapsed() >= WRITE_FAILED_GRACE);
        // Inputs that came before the end still count. What is left in `bytes` now is half a message.
        if (closed || self.left) && self.inputs.is_empty() && self.ended.is_none() {
            self.ended = Some(if self.left { "THE OTHER PLAYER LEFT" } else { "CONNECTION LOST" });
        }
    }

    /// The other computer waits for us: tell it, so that it can say so instead of looking frozen. Does not change the game.
    fn sync_pause(&mut self, blocking: bool) {
        if blocking != self.sent_paused {
            self.sent_paused = blocking;
            self.send(&[MSG_PAUSE, blocking as u8]);
        }
    }

    /// The other player's pause menu holds up the game.
    pub fn peer_paused(&self) -> bool {
        self.peer_paused && self.ended.is_none()
    }

    fn send_check(&mut self, tick: u64, sum: u32) {
        let mut msg = vec![MSG_CHECK];
        msg.extend_from_slice(&tick.to_le_bytes());
        msg.extend_from_slice(&sum.to_le_bytes());
        self.send(&msg);
    }

    fn step(&mut self, game: &mut GameState, input: &ControllerInput) {
        game.update(input);
        self.tick += 1;
        if self.tick % CHECK_EVERY == 0 {
            let sum = checksum(game);
            self.my_checks.push_back((self.tick, sum));
            self.send_check(self.tick, sum);
        }
    }

    /// One logic step when each player has their own game: `mine` plays with the local input unless it is over or `paused`,
    /// `theirs` follows the inputs of the other computer. The checksums of `mine` go out and are compared with their copy there.
    /// Returns the steps of `theirs`.
    pub fn tick_same_time(&mut self, mine: &mut GameState, theirs: &mut GameState, local: &ControllerInput, paused: bool) -> usize {
        self.receive();
        if self.ended.is_some() {
            return 0;
        }
        self.sync_pause(paused && !mine.game_over);
        if !mine.game_over && !paused {
            let bits = input_bits(local);
            self.send(&[MSG_INPUT, bits]);
            mine.update(&input_from_bits(bits));
            self.my_tick += 1;
            if self.my_tick % CHECK_EVERY == 0 {
                self.send_check(self.my_tick, checksum(mine));
            }
        }
        let mut steps = 0;
        let n = self.playout.steps(self.inputs.len(), std::mem::take(&mut self.arrived));
        while steps < n {
            let Some(bits) = self.inputs.pop_front() else { break };
            theirs.update(&input_from_bits(bits));
            self.tick += 1;
            steps += 1;
            if self.tick % CHECK_EVERY == 0 {
                self.my_checks.push_back((self.tick, checksum(theirs)));
            }
        }
        self.compare_checks();
        steps
    }

    fn compare_checks(&mut self) {
        while let (Some(&(a, x)), Some(&(b, y))) = (self.my_checks.front(), self.peer_checks.front()) {
            if a == b {
                if x == y {
                    self.verified += 1;
                } else {
                    self.ended = Some("GAMES OUT OF SYNC");
                }
            }
            if a <= b {
                self.my_checks.pop_front();
            }
            if b <= a {
                self.peer_checks.pop_front();
            }
        }
    }

    pub fn my_turn(&self, game: &GameState) -> bool {
        game.player == self.me
    }

    /// One logic step. On this player's turn the local input plays and goes out, unless `paused`; otherwise the inputs of the
    /// other computer play. Returns how many steps ran.
    pub fn tick(&mut self, game: &mut GameState, local: &ControllerInput, paused: bool) -> usize {
        self.receive();
        if self.ended.is_some() {
            return 0;
        }
        self.sync_pause(paused && self.my_turn(game));
        let mut steps = 0;
        let arrived = std::mem::take(&mut self.arrived);
        if self.my_turn(game) && self.inputs.is_empty() {
            if !paused {
                let bits = input_bits(local);
                self.send(&[MSG_INPUT, bits]);
                self.step(game, &input_from_bits(bits));
                steps = 1;
            }
        } else if self.my_turn(game) {
            // Keep the turn order exact: inputs that arrived are used up first, even if the turn just came here
            if let Some(bits) = self.inputs.pop_front() {
                self.step(game, &input_from_bits(bits));
                steps = 1;
            }
        } else {
            // Watching: the inputs of the other computer go through the buffer that hides network bursts
            let n = self.playout.steps(self.inputs.len(), arrived);
            while steps < n {
                let Some(bits) = self.inputs.pop_front() else { break };
                self.step(game, &input_from_bits(bits));
                steps += 1;
                if self.my_turn(game) {
                    break;
                }
            }
        }
        self.compare_checks();
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays a stream of arrival counts per tick, returns (ticks that stood still while inputs were coming, double steps, left over)
    fn play(p: &mut Playout, arrivals: &[usize]) -> (usize, usize, usize) {
        let (mut q, mut still, mut double) = (0usize, 0, 0);
        for (t, &a) in arrivals.iter().enumerate() {
            q += a;
            let n = p.steps(q, a).min(q);
            q -= n;
            if t > 100 {
                still += (n == 0) as usize;
                double += (n > 1) as usize;
            }
        }
        (still, double, q)
    }

    #[test]
    fn steady_inputs_play_one_step_per_tick() {
        let mut p = Playout::new();
        let (still, double, _) = play(&mut p, &vec![1; 1000]);
        assert_eq!((still, double), (0, 0));
    }

    #[test]
    fn a_bursty_network_does_not_stutter_after_the_first_hiccups() {
        // every 75 ticks the network holds 5 inputs back (100 ms) and delivers them at once
        let mut arrivals = Vec::new();
        for _ in 0..40 {
            arrivals.extend(std::iter::repeat(1).take(70));
            arrivals.extend([0, 0, 0, 0, 0, 6]);
        }
        let mut p = Playout::new();
        let (still, double, _) = play(&mut p, &arrivals);
        // the buffer grows during the first bursts, then it hides them
        let (still_late, double_late, _) = play(&mut p, &arrivals);
        assert!(still + double < 40, "still {still} double {double}");
        assert_eq!((still_late, double_late), (0, 0));
    }

    #[test]
    fn inputs_left_when_the_other_side_stops_are_played() {
        let mut p = Playout::new();
        let mut arrivals = vec![1; 200];
        arrivals.push(0);
        arrivals.extend(vec![0; 40]);
        let (_, _, left) = play(&mut p, &arrivals);
        assert_eq!(left, 0);
        // also right at the start, with fewer inputs than the buffer wants
        let mut p = Playout::new();
        let (_, _, left) = play(&mut p, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(left, 0);
    }

    #[test]
    fn a_long_pause_does_not_grow_the_buffer() {
        let mut p = Playout::new();
        let mut arrivals = vec![1; 200];
        arrivals.extend(vec![0; 300]);
        arrivals.extend(vec![1; 200]);
        play(&mut p, &arrivals);
        assert_eq!(p.target, Playout::START);
    }

    #[test]
    fn every_input_survives_the_wire() {
        for bits in 0..64u8 {
            assert_eq!(input_bits(&input_from_bits(bits)), bits);
        }
    }

    fn pair() -> (Session, TcpStream) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let other = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        (Session::start(stream, 0, NetMode::TakeTurns).unwrap(), other)
    }

    #[test]
    fn a_failed_write_ends_the_connection_and_stops_writing() {
        let (mut s, _other) = pair();
        s.write_failed = Some(Instant::now() - WRITE_FAILED_GRACE);
        s.send(&[MSG_INPUT, 0]); // skipped, the mark stays
        s.receive();
        assert_eq!(s.ended, Some("CONNECTION LOST"));
    }

    #[test]
    fn a_failed_write_leaves_time_for_what_is_on_its_way() {
        let (mut s, mut other) = pair();
        s.write_failed = Some(Instant::now());
        s.receive();
        assert_eq!(s.ended, None);
        other.write_all(&[MSG_INPUT, 3]).unwrap();
        let t = Instant::now();
        while s.inputs.is_empty() && t.elapsed() < Duration::from_secs(2) {
            s.receive();
        }
        assert_eq!(s.inputs.pop_front(), Some(3));
    }

    #[test]
    fn the_pause_message_reaches_the_other_side_and_changes_nothing_else() {
        let (mut s, other) = pair();
        s.sync_pause(true);
        s.sync_pause(true); // sent once
        let mut r = Session::start(other, 1, NetMode::TakeTurns).unwrap();
        let t = Instant::now();
        while !r.peer_paused() && t.elapsed() < Duration::from_secs(2) {
            r.receive();
        }
        assert!(r.peer_paused());
        assert!(r.inputs.is_empty() && r.ended.is_none());
        s.sync_pause(false);
        while r.peer_paused() && t.elapsed() < Duration::from_secs(2) {
            r.receive();
        }
        assert!(!r.peer_paused());
    }

    #[test]
    fn addresses_are_checked_without_looking_anything_up() {
        assert_eq!(parse_address("192.168.1.5").unwrap(), SocketAddr::from(([192, 168, 1, 5], PORT)));
        assert_eq!(parse_address("10.0.0.1:5000").unwrap().port(), 5000);
        for bad in ["", "1.2.3", "example.com", "1.2.3.4:", "1.2.3.4:0", "999.1.1.1"] {
            assert!(matches!(parse_address(bad), Err(NetError::BadAddress)), "{bad}");
        }
    }

    #[test]
    fn errors_have_their_own_texts() {
        use std::io::{Error, ErrorKind};
        assert!(matches!(NetError::from(Error::from(ErrorKind::ConnectionRefused)), NetError::Refused));
        assert!(matches!(NetError::from(Error::from(ErrorKind::TimedOut)), NetError::TimedOut));
        assert!(matches!(NetError::from(Error::from(ErrorKind::WouldBlock)), NetError::TimedOut));
        assert!(matches!(NetError::from(Error::from(ErrorKind::NotFound)), NetError::Other(_)));
        let texts = [NetError::Refused.text(), NetError::TimedOut.text(), NetError::BadAddress.text(), NetError::WrongVersion.text()];
        assert!(texts.iter().all(|t| t.len() <= 36));
        assert_eq!(texts.iter().collect::<std::collections::HashSet<_>>().len(), 4);
    }

    #[test]
    fn nobody_at_the_address_is_refused() {
        let port = TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port(); // closed again
        assert!(matches!(join(&format!("127.0.0.1:{port}")), Err(NetError::Refused)));
    }

    #[test]
    fn input_survives_the_byte() {
        for bits in 0..32u8 {
            assert_eq!(input_bits(&input_from_bits(bits)), bits);
        }
    }
}
