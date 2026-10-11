//! Live sync between real `kuro` processes, each in its own pseudo-terminal, with real sockets, a real file lock
//! and real journals (isolated in a temp dir). Includes killing processes with SIGKILL.

mod common;
use common::pty::*;
use std::path::{Path, PathBuf};

const CFG: &str = "[editor]\nmodal = false\n[clipboard]\nbackend = \"osc52\"\n";

struct Env {
    dir: tempfile::TempDir,
    file: PathBuf,
}

fn env(initial: &str) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("shared.txt");
    std::fs::write(&file, initial).unwrap();
    Env { dir, file }
}

/// Every terminal has its own config dir (they would overwrite each other's file), the same runtime and state.
fn terminal(e: &Env, name: &str) -> Pty {
    let conf = e.dir.path().join(name);
    std::fs::create_dir_all(&conf).unwrap();
    let run = e.dir.path().join("run");
    let state = e.dir.path().join("state");
    Pty::spawn(&conf, CFG, Some(&e.file), &[("XDG_RUNTIME_DIR", run.to_str().unwrap()), ("XDG_STATE_HOME", state.to_str().unwrap())])
}

fn row(s: &[String], needle: &str) -> bool {
    s.iter().any(|r| r.contains(needle))
}

fn journal_contains(state: &Path, needle: &str) -> bool {
    let Ok(rd) = std::fs::read_dir(state.join("kuro/recovery")) else { return false };
    rd.flatten().any(|f| std::fs::read_to_string(f.path()).is_ok_and(|t| t.contains(needle)))
}

#[test]
fn typing_in_one_process_appears_in_the_other_and_a_save_from_either_writes_both() {
    let e = env("alpha\nbeta\n");
    let mut a = terminal(&e, "a");
    a.wait_for("a opens", |s| row(s, "alpha"));
    let mut b = terminal(&e, "b");
    b.wait_for("b opens", |s| row(s, "alpha"));
    assert!(b.screen()[28].contains("sync"), "{:?}", b.screen()[28]);
    a.send("FROM_A ");
    b.wait_for("the edit reaches b", |s| row(s, "FROM_A alpha"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "alpha\nbeta\n", "unsaved: disk is untouched");
    b.send("\x1b[1;5F"); // Ctrl+End
    b.send("FROM_B");
    a.wait_for("the edit reaches a", |s| row(s, "FROM_B"));
    b.send(CTRL_S);
    a.wait_for("a is clean after b saved", |s| !s[28].contains("[+]") && row(s, "written"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "FROM_A alpha\nbeta\nFROM_B");
    a.send(CTRL_Q);
    assert!(a.wait_exit());
    b.send(CTRL_Q);
    assert!(b.wait_exit());
}

#[test]
fn killing_the_leader_process_loses_nothing_and_the_other_terminal_takes_over() {
    let e = env("one\ntwo\n");
    let mut a = terminal(&e, "a");
    a.wait_for("a opens", |s| row(s, "one"));
    let mut b = terminal(&e, "b");
    b.wait_for("b opens", |s| row(s, "one"));
    a.send("A1 ");
    b.wait_for("a's edit reaches b", |s| row(s, "A1 one"));
    b.send("\x1b[1;5F");
    b.send("B1");
    a.wait_for("b's edit reaches a", |s| row(s, "B1"));
    a.sigkill(); // a was the leader: no goodbye, no flush
    // b notices, takes over, and still has everything
    b.wait_for("b is still live after the handover", |s| s[28].contains("sync") && !s[28].contains("re-connecting") && row(s, "A1 one"));
    b.send("B2");
    b.wait_for("editing goes on", |s| row(s, "B1B2"));
    b.send(CTRL_S);
    b.wait_for("saved", |s| row(s, "written"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "A1 one\ntwo\nB1B2");
    // a newcomer joins the new leader and sees the saved text
    let mut c = terminal(&e, "c");
    c.wait_for("c opens", |s| row(s, "A1 one"));
    assert!(c.screen()[28].contains("sync"));
}

#[test]
fn unsaved_edits_survive_every_process_being_killed() {
    let e = env("base\n");
    let mut a = terminal(&e, "a");
    a.wait_for("a opens", |s| row(s, "base"));
    let mut b = terminal(&e, "b");
    b.wait_for("b opens", |s| row(s, "base"));
    a.send("PRECIOUS ");
    b.wait_for("arrives", |s| row(s, "PRECIOUS base"));
    // wait until the leader has written its journal (it flushes about once a second)
    let state = e.dir.path().join("state");
    let t = std::time::Instant::now();
    while !journal_contains(&state, "PRECIOUS") {
        assert!(t.elapsed() < std::time::Duration::from_secs(10), "the journal was never written");
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    a.sigkill();
    b.sigkill();
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "base\n");
    // a fresh start finds the journal
    let mut c = terminal(&e, "c");
    c.wait_for("the unsaved edit is restored", |s| row(s, "PRECIOUS base"));
    c.wait_for("and it is still unsaved", |s| s[28].contains("[+]"));
    c.send(CTRL_S);
    c.wait_for("saved", |s| row(s, "written"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "PRECIOUS base\n");
}
