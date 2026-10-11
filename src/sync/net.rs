//! The real runtime: Unix sockets between terminals, an OS file lock to decide who leads, and a [`SyncHandle`]
//! that owns the [`Node`]. All decisions live in `Node`/`LeaderCore` (tested in the simulator); this module only
//! moves bytes.
//!
//! Threading: the `Node` lives in the caller's thread (the editor's), so a local edit and an incoming remote change
//! are always handled in one strict order; only socket reading/writing, accepting and leader elections run on
//! helper threads, which post events that `poll` handles.
//!
//! Layout per file (key = hash of its canonical path), under the runtime directory (mode 0700):
//! * `<key>.lock`: held with `flock` by the leader. The kernel releases it when the leader process dies, however it dies.
//! * `<key>.sock`: the leader's listening socket.
//!
//! Journals live under the state directory: `recovery/<key>.journal`.

use super::node::{Input, Node, Output, State};
use super::ot::Op;
use super::protocol::{ClientId, ClientMsg, ServerMsg, decode, encode};
use super::real::{FileJournal, RealFs};
use super::replica::Effect;
use super::storage::{Fs, Journal, hash_bytes};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const TICK_MS: u64 = 100;
const JOIN_TIMEOUT: Duration = Duration::from_secs(3);
const ELECTION_ROUNDS: usize = 5;

#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub runtime_dir: PathBuf,
    pub state_dir: PathBuf,
    /// Files bigger than this are not synced.
    pub max_file_bytes: u64,
    /// Fixed terminal id (tests); random when `None`.
    pub client_id: Option<ClientId>,
}

impl SyncOptions {
    pub fn from_env(max_file_bytes: u64) -> SyncOptions {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let uid = unsafe { libc::geteuid() };
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()).map(|d| PathBuf::from(d).join("kuro")).unwrap_or_else(|| PathBuf::from(format!("/tmp/kuro-{uid}")));
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
        let state = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()).map(|d| PathBuf::from(d).join("kuro")).unwrap_or_else(|| home.join(".local/state/kuro"));
        SyncOptions { runtime_dir: runtime, state_dir: state, max_file_bytes, client_id: None }
    }
}

#[derive(Debug)]
pub enum StartError {
    /// The file is bigger than `sync.max_file_mib`.
    TooLarge(u64),
    NotUtf8,
    Io(String),
    /// Nobody leads and this terminal could not take the lock either.
    NoLeader,
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::TooLarge(n) => write!(f, "file is {:.1} MiB", *n as f64 / 1048576.0),
            StartError::NotUtf8 => write!(f, "file is not valid UTF-8"),
            StartError::Io(e) => write!(f, "{e}"),
            StartError::NoLeader => write!(f, "could not find or start a sync leader"),
        }
    }
}

/// What the editor needs to hear about.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncEvent {
    Editor(Effect),
    Notice(String),
    /// Sync state or role changed.
    Status { state: State, leader: bool },
}

/// `<key>` for a file: the first 16 hex digits of the hash of its canonical path.
pub fn key_for(path: &Path) -> String {
    hash_bytes(path.to_string_lossy().as_bytes())[..16].to_string()
}

/// How a leader election ended (computed on a helper thread).
enum Election {
    Won(File),
    Connected(UnixStream),
    Failed,
}

enum Event {
    /// A connection produced a line, or closed.
    Wire(u64, Option<String>),
    Accepted(u64, UnixStream),
    Election(Election),
}

pub struct SyncHandle {
    pub initial_text: String,
    pub client_id: ClientId,
    core: Core,
}

impl SyncHandle {
    /// Join (or start) the sync group for `path`. `wake` is called (from helper threads) whenever there is
    /// something new for [`SyncHandle::poll`] to handle.
    pub fn start(path: &Path, opts: SyncOptions, wake: Arc<dyn Fn() + Send + Sync>) -> Result<SyncHandle, StartError> {
        let canonical = fs::canonicalize(path).or_else(|_| {
            // a file that does not exist yet: canonicalise its folder
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
            fs::canonicalize(dir).map(|d| d.join(path.file_name().unwrap_or_default()))
        });
        let canonical = canonical.map_err(|e| StartError::Io(e.to_string()))?;
        if let Ok(m) = fs::metadata(&canonical) {
            if m.len() > opts.max_file_bytes {
                return Err(StartError::TooLarge(m.len()));
            }
        }
        let client_id = opts.client_id.unwrap_or_else(random_id);
        let (tx, rx) = mpsc::channel::<Event>();
        let mut core = Core::new(canonical, opts, client_id, tx, rx, wake);
        let initial_text = core.join()?;
        Ok(SyncHandle { initial_text, client_id, core })
    }

    pub fn is_leader(&self) -> bool {
        self.core.node.as_ref().is_some_and(|n| n.is_leader())
    }

    pub fn state(&self) -> State {
        self.core.node.as_ref().map(|n| n.state).unwrap_or(State::Standalone)
    }

    /// The editor changed its text with `ops`. Call this right after applying them locally.
    pub fn local(&mut self, ops: Vec<Op>) {
        self.core.input(Input::Local(ops));
    }
    pub fn undo(&mut self) {
        self.core.input(Input::LocalUndo);
    }
    pub fn redo(&mut self) {
        self.core.input(Input::LocalRedo);
    }
    pub fn save(&mut self, force: bool) {
        self.core.input(Input::LocalSave { force });
    }

    /// Handle everything that arrived from the network and do the periodic work; returns what the editor must
    /// show or apply, **in order**.
    pub fn poll(&mut self) -> Vec<SyncEvent> {
        self.core.poll()
    }

    /// Leave cleanly: flush the journal, say goodbye, stop.
    pub fn shutdown(mut self) {
        self.core.graceful_stop();
    }

    /// Simulate the process dying: nothing is flushed, nothing is announced; the OS-level effects (closed
    /// sockets, released lock) are the same as for a killed process.
    pub fn crash(mut self) {
        self.core.die();
    }
}

impl Drop for SyncHandle {
    fn drop(&mut self) {
        if self.core.node.is_some() {
            self.core.graceful_stop();
        }
    }
}

fn random_id() -> ClientId {
    let mut b = [0u8; 8];
    if File::open("/dev/urandom").and_then(|mut f| io::Read::read_exact(&mut f, &mut b)).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        b = (t ^ ((std::process::id() as u64) << 32)).to_le_bytes();
    }
    (u64::from_le_bytes(b) >> 1).max(1)
}

// ---------------------------------------------------------------------- OS helpers

/// Take the leader lock without blocking. `Ok(None)` when someone else holds it.
fn try_lock(path: &Path) -> io::Result<Option<File>> {
    let f = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
    // SAFETY: flock on a valid, open file descriptor owned by `f`.
    let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if r == 0 {
        return Ok(Some(f));
    }
    let e = io::Error::last_os_error();
    if e.raw_os_error() == Some(libc::EWOULDBLOCK) { Ok(None) } else { Err(e) }
}

/// The user id of the process on the other end of a Unix socket.
fn peer_uid(s: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: SO_PEERCRED writes a `ucred` into the buffer we pass, whose size we give in `len`.
    let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len) };
    (r == 0).then_some(cred.uid)
}

fn private_dir(d: &Path) -> io::Result<()> {
    fs::create_dir_all(d)?;
    fs::set_permissions(d, fs::Permissions::from_mode(0o700))
}

// ---------------------------------------------------------------------- the core (lives on the caller's thread)

struct ConnWriter {
    tx: Sender<String>,
    stream: UnixStream,
}

impl ConnWriter {
    fn send(&self, line: String) {
        let _ = self.tx.send(line);
    }
    fn close(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

struct Core {
    path: PathBuf,
    opts: SyncOptions,
    id: ClientId,
    key: String,
    node: Option<Node>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    /// What the editor has not heard yet.
    up: Vec<SyncEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
    started: Instant,
    last_tick_ms: u64,
    next_conn: u64,
    /// Connections we serve (leader) or the one to our leader (follower), by connection number.
    conns: HashMap<u64, ConnWriter>,
    /// Leader: which terminal is on which connection.
    client_conn: HashMap<ClientId, u64>,
    conn_client: HashMap<u64, ClientId>,
    leader_conn: Option<u64>,
    lock: Option<File>,
    stop_accept: Arc<AtomicBool>,
    accept_thread: Option<JoinHandle<()>>,
    listener_path: Option<PathBuf>,
    electing: bool,
}

impl Core {
    fn new(path: PathBuf, opts: SyncOptions, id: ClientId, tx: Sender<Event>, rx: Receiver<Event>, wake: Arc<dyn Fn() + Send + Sync>) -> Core {
        let key = key_for(&path);
        Core {
            path,
            opts,
            id,
            key,
            node: None,
            tx,
            rx,
            up: vec![],
            wake,
            started: Instant::now(),
            last_tick_ms: 0,
            next_conn: 1,
            conns: HashMap::new(),
            client_conn: HashMap::new(),
            conn_client: HashMap::new(),
            leader_conn: None,
            lock: None,
            stop_accept: Arc::new(AtomicBool::new(false)),
            accept_thread: None,
            listener_path: None,
            electing: false,
        }
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn lock_path(&self) -> PathBuf {
        self.opts.runtime_dir.join(format!("{}.lock", self.key))
    }
    fn sock_path(&self) -> PathBuf {
        self.opts.runtime_dir.join(format!("{}.sock", self.key))
    }
    fn journal_path(&self) -> PathBuf {
        self.opts.state_dir.join("recovery").join(format!("{}.journal", self.key))
    }

    fn factories(&self) -> (super::node::FsFactory, super::node::JournalFactory) {
        let jp = self.journal_path();
        (Box::new(|| Box::new(RealFs) as Box<dyn Fs>), Box::new(move || Box::new(FileJournal::new(jp.clone())) as Box<dyn Journal>))
    }

    // ---- joining

    /// Become the leader or join an existing one; returns the shared text.
    fn join(&mut self) -> Result<String, StartError> {
        private_dir(&self.opts.runtime_dir).map_err(|e| StartError::Io(format!("runtime directory: {e}")))?;
        let deadline = Instant::now() + JOIN_TIMEOUT;
        loop {
            if self.connect_to_leader().is_ok() {
                return self.finish_follower_join();
            }
            match try_lock(&self.lock_path()) {
                Ok(Some(lock)) => return self.become_first_leader(lock),
                Ok(None) => {} // someone is starting up: retry connecting
                Err(e) => return Err(StartError::Io(format!("lock: {e}"))),
            }
            if Instant::now() > deadline {
                return Err(StartError::NoLeader);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn become_first_leader(&mut self, lock: File) -> Result<String, StartError> {
        let (ff, jf) = self.factories();
        let now = self.now_ms();
        let (node, outs) = Node::new_leader(self.id, self.path.clone(), ff, jf, now).map_err(|e| {
            if e.kind() == io::ErrorKind::InvalidData { StartError::NotUtf8 } else { StartError::Io(e.to_string()) }
        })?;
        self.lock = Some(lock);
        self.node = Some(node);
        self.start_listening().map_err(|e| StartError::Io(format!("socket: {e}")))?;
        let mut text = None;
        for o in outs {
            match o {
                Output::Editor(Effect::Replace(t)) => text = Some(t),
                other => self.perform_one(other),
            }
        }
        self.status();
        text.ok_or_else(|| StartError::Io("the leader did not produce the initial text".into()))
    }

    fn connect_to_leader(&mut self) -> io::Result<u64> {
        let s = UnixStream::connect(self.sock_path())?;
        let conn = self.next_conn;
        self.next_conn += 1;
        self.attach(conn, s);
        self.leader_conn = Some(conn);
        Ok(conn)
    }

    /// First contact with a leader: say hello and wait for the shared text.
    fn finish_follower_join(&mut self) -> Result<String, StartError> {
        let (ff, jf) = self.factories();
        let mut node = Node::follower(self.id, self.path.clone(), ff, jf);
        let hello = node.input(Input::Connected);
        self.node = Some(node);
        for o in hello {
            self.perform_one(o);
        }
        let deadline = Instant::now() + JOIN_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(Event::Wire(c, Some(line))) if Some(c) == self.leader_conn => {
                    let Ok(msg) = decode::<ServerMsg>(&line) else { continue };
                    if let ServerMsg::Welcome { text, .. } = &msg {
                        if text.len() as u64 > self.opts.max_file_bytes {
                            let n = text.len() as u64;
                            self.say_goodbye();
                            self.close_all();
                            return Err(StartError::TooLarge(n));
                        }
                    }
                    let outs = self.node.as_mut().unwrap().input(Input::FromLeader(msg));
                    let mut text = None;
                    for o in outs {
                        match o {
                            Output::Editor(Effect::Replace(t)) => text = Some(t),
                            other => self.perform_one(other),
                        }
                    }
                    if let Some(t) = text {
                        self.status();
                        return Ok(t);
                    }
                }
                Ok(Event::Wire(c, None)) if Some(c) == self.leader_conn => return Err(StartError::NoLeader),
                Ok(_) => {}
                Err(_) => return Err(StartError::NoLeader),
            }
        }
    }

    // ---- sockets and threads

    fn attach(&mut self, id: u64, stream: UnixStream) {
        let (wtx, wrx) = mpsc::channel::<String>();
        if let Ok(mut w) = stream.try_clone() {
            std::thread::spawn(move || {
                for line in wrx {
                    if w.write_all(line.as_bytes()).and_then(|_| w.write_all(b"\n")).is_err() {
                        break;
                    }
                }
            });
        }
        if let Ok(r) = stream.try_clone() {
            let tx = self.tx.clone();
            let wake = self.wake.clone();
            std::thread::spawn(move || {
                let mut rd = BufReader::new(r);
                loop {
                    let mut line = String::new();
                    match rd.read_line(&mut line) {
                        Ok(0) | Err(_) => {
                            let _ = tx.send(Event::Wire(id, None));
                            wake();
                            break;
                        }
                        Ok(_) => {
                            let _ = tx.send(Event::Wire(id, Some(line.trim_end().to_string())));
                            wake();
                        }
                    }
                }
            });
        }
        self.conns.insert(id, ConnWriter { tx: wtx, stream });
    }

    fn start_listening(&mut self) -> io::Result<()> {
        let sock = self.sock_path();
        let _ = fs::remove_file(&sock); // a stale file from a dead leader
        let listener = UnixListener::bind(&sock)?;
        fs::set_permissions(&sock, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        self.listener_path = Some(sock);
        let stop = Arc::new(AtomicBool::new(false));
        self.stop_accept = stop.clone();
        let tx = self.tx.clone();
        let wake = self.wake.clone();
        // SAFETY: geteuid has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        let mut next = 1_000_000u64; // connection numbers for accepted sockets never collide with ours
        self.accept_thread = Some(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((s, _)) => {
                        let _ = s.set_nonblocking(false);
                        if peer_uid(&s).is_some_and(|u| u == me) {
                            next += 1;
                            let _ = tx.send(Event::Accepted(next, s));
                            wake();
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(15)),
                    Err(_) => break,
                }
            }
        }));
        Ok(())
    }

    fn stop_listening(&mut self) {
        self.stop_accept.store(true, Ordering::Relaxed);
        if let Some(t) = self.accept_thread.take() {
            let _ = t.join();
        }
        if let Some(p) = self.listener_path.take() {
            let _ = fs::remove_file(p);
        }
    }

    fn close_all(&mut self) {
        for c in self.conns.values() {
            c.close();
        }
        self.conns.clear();
        self.client_conn.clear();
        self.conn_client.clear();
        self.leader_conn = None;
    }

    fn status(&mut self) {
        if let Some(n) = &self.node {
            self.up.push(SyncEvent::Status { state: n.state, leader: n.is_leader() });
        }
    }

    fn say_goodbye(&mut self) {
        if let Some(c) = self.leader_conn.and_then(|c| self.conns.get(&c)) {
            c.send(encode(&ClientMsg::Bye { client: self.id }));
        }
    }

    // ---- driving the node

    fn input(&mut self, i: Input) {
        let outs = self.node.as_mut().map(|n| n.input(i)).unwrap_or_default();
        self.perform(outs);
    }

    /// Handle everything pending and do the periodic work.
    fn poll(&mut self) -> Vec<SyncEvent> {
        while let Ok(ev) = self.rx.try_recv() {
            let outs = match ev {
                Event::Accepted(id, s) => {
                    self.attach(id, s);
                    vec![]
                }
                Event::Wire(conn, Some(line)) => self.on_line(conn, line),
                Event::Wire(conn, None) => self.on_closed(conn),
                Event::Election(e) => self.on_election(e),
            };
            self.perform(outs);
        }
        let now = self.now_ms();
        if now.saturating_sub(self.last_tick_ms) >= TICK_MS {
            self.last_tick_ms = now;
            self.input(Input::Tick(now));
        }
        std::mem::take(&mut self.up)
    }

    fn on_line(&mut self, conn: u64, line: String) -> Vec<Output> {
        let Some(node) = self.node.as_mut() else { return vec![] };
        if node.is_leader() {
            let Ok(msg) = decode::<ClientMsg>(&line) else { return vec![] };
            let from = match &msg {
                ClientMsg::Hello { client } | ClientMsg::Bye { client } | ClientMsg::Rejoin { client, .. } => {
                    self.client_conn.insert(*client, conn);
                    self.conn_client.insert(conn, *client);
                    *client
                }
                ClientMsg::Edit { id, .. } | ClientMsg::Undo { id } | ClientMsg::Redo { id } | ClientMsg::Save { id, .. } => id.client,
            };
            if matches!(msg, ClientMsg::Bye { .. }) {
                self.drop_conn(conn);
                return self.node.as_mut().unwrap().input(Input::ClientGone(from));
            }
            self.node.as_mut().unwrap().input(Input::FromClient { from, msg })
        } else if Some(conn) == self.leader_conn {
            match decode::<ServerMsg>(&line) {
                Ok(msg) => node.input(Input::FromLeader(msg)),
                Err(_) => vec![],
            }
        } else {
            vec![]
        }
    }

    fn drop_conn(&mut self, conn: u64) {
        if let Some(c) = self.conns.remove(&conn) {
            c.close();
        }
        if let Some(client) = self.conn_client.remove(&conn) {
            self.client_conn.remove(&client);
        }
    }

    fn on_closed(&mut self, conn: u64) -> Vec<Output> {
        let was_leader_conn = self.leader_conn == Some(conn);
        let client = self.conn_client.get(&conn).copied();
        self.drop_conn(conn);
        let Some(node) = self.node.as_mut() else { return vec![] };
        if was_leader_conn {
            self.leader_conn = None;
            return node.input(Input::LeaderLost);
        }
        match client {
            Some(c) => node.input(Input::ClientGone(c)),
            None => vec![],
        }
    }

    fn perform(&mut self, outs: Vec<Output>) {
        let mut changed = false;
        for o in outs {
            changed |= matches!(o, Output::TryElection);
            self.perform_one(o);
        }
        if changed {
            self.status();
        }
    }

    fn perform_one(&mut self, o: Output) {
        match o {
            Output::ToLeader(msg) => {
                if let Some(c) = self.leader_conn.and_then(|c| self.conns.get(&c)) {
                    c.send(encode(&msg));
                }
            }
            Output::ToClient(client, msg) => {
                if let Some(c) = self.client_conn.get(&client).and_then(|c| self.conns.get(c)) {
                    c.send(encode(&msg));
                }
            }
            Output::Editor(e) => self.up.push(SyncEvent::Editor(e)),
            Output::Notice(n) => self.up.push(SyncEvent::Notice(n)),
            Output::TryElection => self.spawn_election(),
        }
    }

    // ---- leader change

    /// The leader connection closed: on a helper thread, take over if the lock is free, otherwise join whoever has it.
    fn spawn_election(&mut self) {
        if self.electing {
            return;
        }
        self.electing = true;
        let (lock_path, sock_path) = (self.lock_path(), self.sock_path());
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let mut result = Election::Failed;
            'rounds: for round in 0..ELECTION_ROUNDS {
                match try_lock(&lock_path) {
                    Ok(Some(lock)) => {
                        result = Election::Won(lock);
                        break;
                    }
                    Ok(None) => {
                        // someone else leads (or is about to): connect, giving a starting leader time to bind
                        let until = Instant::now() + Duration::from_millis(900);
                        while Instant::now() < until {
                            if let Ok(s) = UnixStream::connect(&sock_path) {
                                result = Election::Connected(s);
                                break 'rounds;
                            }
                            std::thread::sleep(Duration::from_millis(30));
                        }
                    }
                    Err(_) => {}
                }
                std::thread::sleep(Duration::from_millis(100 * (round as u64 + 1)));
            }
            let _ = tx.send(Event::Election(result));
            wake();
        });
    }

    fn on_election(&mut self, e: Election) -> Vec<Output> {
        self.electing = false;
        let Some(node) = self.node.as_mut() else { return vec![] };
        match e {
            Election::Won(lock) => {
                self.lock = Some(lock);
                if self.start_listening().is_err() {
                    self.lock = None;
                    return self.node.as_mut().unwrap().input(Input::ElectionFailed);
                }
                self.node.as_mut().unwrap().promote()
            }
            Election::Connected(s) => {
                let _ = &node;
                let conn = self.next_conn;
                self.next_conn += 1;
                self.attach(conn, s);
                self.leader_conn = Some(conn);
                self.node.as_mut().unwrap().input(Input::Connected)
            }
            Election::Failed => node.input(Input::ElectionFailed),
        }
    }

    // ---- leaving

    fn graceful_stop(&mut self) {
        if let Some(n) = self.node.as_mut() {
            if let Some(l) = n.leader.as_mut() {
                l.flush_journal(0);
            }
        }
        self.say_goodbye();
        self.stop_listening();
        self.close_all();
        self.lock = None;
        self.node = None;
    }

    /// Like a killed process: no flush, no goodbye. Sockets close and the lock is released, as the OS would do.
    fn die(&mut self) {
        self.stop_accept.store(true, Ordering::Relaxed);
        if let Some(t) = self.accept_thread.take() {
            let _ = t.join();
        }
        // the listening socket file would stay behind after a real kill: leave it, too
        self.close_all();
        if let Some(n) = self.node.take() {
            std::mem::forget(n); // buffered, un-flushed journal data is lost, like in a real crash
        }
        self.lock = None;
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.stop_accept.store(true, Ordering::Relaxed);
    }
}
