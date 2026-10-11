//! The editor on top of live sync: several `App`s (each on its own thread, like separate terminals) with
//! one real file, real sockets and real journals in a temp dir. Nothing here touches the user's real files.

mod common;
use common::ui::*;
use kuro::app::App;
use kuro::config::Config;
use kuro::sync::net::SyncOptions;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

struct Env {
    _dir: tempfile::TempDir,
    runtime: PathBuf,
    state: PathBuf,
    file: PathBuf,
}

fn env(initial: &str) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("work.txt");
    std::fs::write(&file, initial).unwrap();
    Env { runtime: dir.path().join("run"), state: dir.path().join("state"), file, _dir: dir }
}

#[derive(Debug, Clone, Default)]
struct Snap {
    text: String,
    modified: bool,
    cursor: usize,
    top_line: usize,
    label: Option<&'static str>,
    msg: String,
    quit: bool,
    confirm: bool,
}

enum Cmd {
    Typed(String),
    Press(String),
    Run(String),
    /// Pretend to be connected to a database that records what it is asked to run.
    Attach(common::fakedb::Log),
    Cursor(usize),
    Top(usize),
    Snap(Sender<Snap>),
    Stop,
}

/// One terminal: an App on its own thread that keeps ticking (as the real event loop does).
struct Term {
    tx: Sender<Cmd>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Term {
    fn open(e: &Env, id: u64, max_bytes: u64) -> Term {
        Term::open_path(e, &e.file, id, max_bytes)
    }

    fn open_path(e: &Env, file: &Path, id: u64, max_bytes: u64) -> Term {
        let (tx, rx): (Sender<Cmd>, Receiver<Cmd>) = channel();
        let (ready_tx, ready_rx) = channel();
        let opts =
            SyncOptions { runtime_dir: e.runtime.clone(), state_dir: e.state.clone(), max_file_bytes: max_bytes, client_id: Some(id) };
        let file = file.to_path_buf();
        let join = std::thread::spawn(move || {
            let mut cfg = Config::default();
            cfg.editor.modal = false;
            cfg.sql.connections_file = "/nonexistent-kuro-test-dir/connections.toml".into();
            let mut app = App::new(cfg, kuro::buffer::Buffer::new(), Box::new(kuro::clipboard::MemoryClipboard::default()));
            app.set_sync_options(opts);
            app.open_file(&file);
            ready_tx.send(()).unwrap();
            loop {
                match rx.recv_timeout(Duration::from_millis(2)) {
                    Ok(Cmd::Typed(s)) => typed(&mut app, &s),
                    Ok(Cmd::Press(k)) => press(&mut app, &k),
                    Ok(Cmd::Run(c)) => app.exec_command(&c),
                    Ok(Cmd::Attach(log)) => {
                        let db = common::fakedb::Recorder { engine: kuro::sql::profile::Engine::Postgres, log };
                        app.sql.session = Some(kuro::sql::Session::new(Box::new(db)));
                    }
                    Ok(Cmd::Cursor(c)) => app.buffer.cursor = c,
                    Ok(Cmd::Top(t)) => app.top_line = t,
                    Ok(Cmd::Snap(reply)) => {
                        app.tick();
                        let _ = reply.send(Snap {
                            text: app.buffer.text(),
                            modified: app.buffer.is_modified(),
                            cursor: app.buffer.cursor,
                            top_line: app.top_line,
                            label: app.sync_label(),
                            msg: app.msg.clone().unwrap_or_default(),
                            quit: app.quit,
                            confirm: app.confirm.is_some(),
                        });
                    }
                    Ok(Cmd::Stop) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
                app.tick();
            }
        });
        ready_rx.recv_timeout(Duration::from_secs(15)).expect("the terminal opened its file");
        Term { tx, join: Some(join) }
    }

    fn typed(&self, s: &str) {
        self.tx.send(Cmd::Typed(s.into())).unwrap();
    }
    fn press(&self, k: &str) {
        self.tx.send(Cmd::Press(k.into())).unwrap();
    }
    fn run(&self, c: &str) {
        self.tx.send(Cmd::Run(c.into())).unwrap();
    }
    fn snap(&self) -> Snap {
        let (tx, rx) = channel();
        self.tx.send(Cmd::Snap(tx)).unwrap();
        rx.recv_timeout(Duration::from_secs(10)).expect("the terminal answers")
    }
    /// Wait (no fixed sleep) until `cond` holds for this terminal.
    fn wait(&self, what: &str, cond: impl Fn(&Snap) -> bool) -> Snap {
        let t = Instant::now();
        loop {
            let s = self.snap();
            if cond(&s) {
                return s;
            }
            assert!(t.elapsed() < Duration::from_secs(15), "timed out waiting for: {what}; last state: {s:?}");
            std::thread::yield_now();
        }
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

const MAX: u64 = 5 << 20;

#[test]
fn an_unsaved_edit_shows_up_in_the_other_terminals_and_not_on_disk() {
    let e = env("alpha\nbeta\n");
    let a = Term::open(&e, 1, MAX);
    let b = Term::open(&e, 2, MAX);
    assert_eq!(b.snap().label, Some("sync"));
    a.typed("NEW ");
    let seen = b.wait("the edit arrives", |s| s.text.starts_with("NEW alpha"));
    assert!(seen.modified, "unsaved edits mark every view modified");
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "alpha\nbeta\n", "nothing is on disk before a save");
}

#[test]
fn saving_in_one_terminal_writes_everyones_edits_and_clears_modified_everywhere() {
    let e = env("one\ntwo\nthree\n");
    let a = Term::open(&e, 1, MAX);
    let b = Term::open(&e, 2, MAX);
    let c = Term::open(&e, 3, MAX);
    a.typed("A");
    b.wait("A arrives", |s| s.text.starts_with("Aone"));
    b.press("ctrl+end");
    b.typed("B");
    c.wait("both edits arrive", |s| s.text.starts_with("Aone") && s.text.ends_with("B"));
    c.press("ctrl+s");
    for t in [&a, &b, &c] {
        t.wait("saved and clean", |s| !s.modified);
    }
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "Aone\ntwo\nthree\nB");
}

#[test]
fn undo_takes_back_the_last_edit_in_the_shared_document_whoever_made_it() {
    let e = env("");
    let a = Term::open(&e, 1, MAX);
    let b = Term::open(&e, 2, MAX);
    a.typed("first ");
    b.wait("first arrives", |s| s.text == "first ");
    b.press("ctrl+end");
    b.typed("second");
    a.wait("second arrives", |s| s.text == "first second");
    a.press("ctrl+z");
    for t in [&a, &b] {
        t.wait("the last edit is undone everywhere", |s| s.text == "first ");
    }
    b.press("ctrl+shift+z");
    for t in [&a, &b] {
        t.wait("and redone everywhere", |s| s.text == "first second");
    }
}

#[test]
fn the_cursor_stays_on_the_same_text_when_another_terminal_edits_above_it() {
    let e = env("one\ntwo\nthree\n");
    let a = Term::open(&e, 1, MAX);
    let b = Term::open(&e, 2, MAX);
    a.tx.send(Cmd::Cursor(8)).unwrap(); // on the 't' of "three"
    b.typed("inserted line\n");
    let s = a.wait("the insert arrives", |s| s.text.starts_with("inserted line\none"));
    assert_eq!(&s.text[s.cursor..s.cursor + 5], "three", "the cursor followed its text");
}

#[test]
fn the_view_keeps_showing_the_same_lines_when_text_is_added_above() {
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let e = env(&text);
    let a = Term::open(&e, 1, MAX);
    let b = Term::open(&e, 2, MAX);
    a.tx.send(Cmd::Top(100)).unwrap();
    b.typed("x\ny\nz\n");
    let s = a.wait("the insert arrives", |s| s.text.starts_with("x\ny\nz\nline 0"));
    assert_eq!(s.top_line, 103, "three lines were added above the view");
}

#[test]
fn a_file_over_the_limit_opens_without_sync_and_says_so() {
    let e = env(&"x".repeat(2000));
    let a = Term::open(&e, 1, 1000);
    let s = a.snap();
    assert_eq!(s.label, Some("local"));
    assert!(s.msg.contains("without live sync"), "{}", s.msg);
    a.typed("y");
    assert!(a.snap().modified);
    a.press("ctrl+s");
    a.wait("a plain local save works", |s| !s.modified);
    assert!(std::fs::read_to_string(&e.file).unwrap().starts_with('y'));
}

#[test]
fn a_crlf_file_opens_without_sync_and_keeps_its_line_endings() {
    let e = env("a\r\nb\r\n");
    let a = Term::open(&e, 1, MAX);
    assert_eq!(a.snap().label, Some("local"));
    a.typed("z");
    a.press("ctrl+s");
    a.wait("saved", |s| !s.modified);
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "za\r\nb\r\n");
}

#[test]
fn quitting_asks_when_another_terminal_has_unsaved_edits_in_the_shared_file() {
    let e = env("data\n");
    let a = Term::open(&e, 1, MAX);
    let b = Term::open(&e, 2, MAX);
    a.typed("!");
    b.wait("the edit arrives", |s| s.modified);
    b.press("ctrl+q");
    assert!(b.wait("a question appears", |s| s.confirm).confirm);
    b.press("y");
    let s = b.wait("saved then quit", |s| s.quit);
    assert!(s.quit);
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "!data\n");
}

#[test]
fn the_other_terminals_carry_on_when_the_leader_terminal_closes() {
    let e = env("keep\n");
    let a = Term::open(&e, 1, MAX);
    let b = Term::open(&e, 2, MAX);
    a.typed("1");
    b.wait("1 arrives", |s| s.text == "1keep\n");
    drop(a); // the leader's terminal closes (graceful)
    b.press("ctrl+end");
    b.typed("2");
    b.wait("still editable", |s| s.text == "1keep\n2");
    b.press("ctrl+s");
    b.wait("saves after the handover", |s| !s.modified);
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "1keep\n2");
}

#[test]
fn colon_w_and_colon_wq_work_with_sync() {
    let e = env("x\n");
    let a = Term::open(&e, 1, MAX);
    a.typed("1");
    a.run("w");
    a.wait("written", |s| !s.modified);
    a.typed("2");
    a.run("wq");
    let s = a.wait("quits after the save", |s| s.quit);
    assert!(s.quit);
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "12x\n");
}

#[test]
fn a_sql_file_open_in_two_terminals_is_one_shared_buffer_and_runs_the_shared_text() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("queries.sql");
    std::fs::write(&file, "select 1;\n").unwrap();
    let e = Env { runtime: dir.path().join("run"), state: dir.path().join("state"), file: file.clone(), _dir: dir };
    let a = Term::open(&e, 1, MAX);
    let b = Term::open(&e, 2, MAX);
    // a SQL file is read-only until connected (disconnected state), so both terminals connect first
    let log_a: common::fakedb::Log = Default::default();
    a.tx.send(Cmd::Attach(log_a)).unwrap();
    a.press("ctrl+end");
    a.typed("select 2;");
    b.wait("the new statement reaches b", |s| s.text == "select 1;\nselect 2;");
    let log: common::fakedb::Log = Default::default();
    b.tx.send(Cmd::Attach(log.clone())).unwrap();
    b.press("ctrl+end");
    b.run("run");
    let t = Instant::now();
    while log.lock().unwrap().is_empty() {
        b.snap(); // keeps the terminal ticking so the statement is sent
        assert!(t.elapsed() < Duration::from_secs(10), "nothing was run");
        std::thread::yield_now();
    }
    assert_eq!(log.lock().unwrap().as_slice(), ["select 2;"], "b ran the statement a typed");
}
