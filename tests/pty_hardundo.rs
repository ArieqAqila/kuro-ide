//! Hard undo with the real binary: real processes, SIGKILL, a second terminal on the same file. Everything under a throw-away HOME.

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
    let file = dir.path().join("work.txt");
    std::fs::write(&file, initial).unwrap();
    Env { dir, file }
}

/// One terminal: its own config folder, the shared state / runtime / data folders.
fn terminal(e: &Env, name: &str, cfg: &str) -> Pty {
    let conf = e.dir.path().join(name);
    std::fs::create_dir_all(&conf).unwrap();
    let st = e.dir.path().join("state");
    let run = e.dir.path().join("run");
    let data = e.dir.path().join("data");
    Pty::spawn(
        &conf,
        cfg,
        Some(&e.file),
        &[("XDG_STATE_HOME", st.to_str().unwrap()), ("XDG_RUNTIME_DIR", run.to_str().unwrap()), ("XDG_DATA_HOME", data.to_str().unwrap())],
    )
}

fn has(s: &[String], needle: &str) -> bool {
    s.iter().any(|r| r.contains(needle))
}

fn command(pty: &mut Pty, cmd: &str) {
    pty.send("\x10"); // Ctrl+P: the command line (the editor is modeless here)
    pty.send(&format!("{cmd}\r"));
}

#[test]
fn hard_undo_works_after_the_editor_was_killed_and_started_again() {
    let e = env("the original text\n");
    let mut a = terminal(&e, "a", CFG);
    a.wait_for("opened", |s| has(s, "the original text"));
    a.send("EDITED ");
    a.send(CTRL_S);
    a.wait_for("saved", |s| has(s, "written"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "EDITED the original text\n");
    a.sigkill();
    // a new process, much later
    let mut b = terminal(&e, "b", CFG);
    b.wait_for("opened", |s| has(s, "EDITED the original text"));
    command(&mut b, "hardundo");
    let s = b.wait_for("the question", |s| has(s, "Hard undo: restore 1 file"));
    assert!(has(&s, "work.txt") && has(&s, "1 change"), "{}", s.join("\n"));
    b.send("y");
    b.wait_for("restored", |s| has(s, "the original text") && !has(s, "EDITED") && !has(s, "Hard undo: restore"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "the original text\n");
    b.assert_default_background();
    b.assert_ansi_palette_only();
}

#[test]
fn cancelling_leaves_the_file_alone_in_the_real_binary() {
    let e = env("orig\n");
    let mut a = terminal(&e, "a", CFG);
    a.wait_for("opened", |s| has(s, "orig"));
    a.send("X");
    a.send(CTRL_S);
    a.wait_for("saved", |s| has(s, "written"));
    command(&mut a, "hardundo");
    a.wait_for("the question", |s| has(s, "[y] restore"));
    a.send("n");
    a.wait_for("cancelled", |s| has(s, "cancelled"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "Xorig\n");
}

#[test]
fn saves_made_by_the_sync_leader_are_recorded_and_every_terminal_follows_a_hard_undo() {
    let e = env("shared original\n");
    let mut a = terminal(&e, "a", CFG); // the leader
    a.wait_for("a opened", |s| has(s, "shared original"));
    let mut b = terminal(&e, "b", CFG);
    b.wait_for("b opened", |s| has(s, "shared original") && has(s, "sync"));
    // b types and saves: the leader (a) writes the file
    b.send("FROM_B ");
    a.wait_for("the edit reaches a", |s| has(s, "FROM_B shared original"));
    b.send(CTRL_S);
    a.wait_for("a is clean after the save", |s| !has(s, "[+]") && has(s, "FROM_B"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "FROM_B shared original\n");
    // a runs the hard undo; b follows
    command(&mut a, "hardundo");
    a.wait_for("the question", |s| has(s, "Hard undo: restore 1 file"));
    a.send("y");
    a.wait_for("restored", |s| has(s, "1 file restored"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "shared original\n");
    b.wait_for("b follows", |s| has(s, "shared original") && !has(s, "FROM_B"));
}

#[test]
fn a_second_question_appears_when_another_editor_changed_the_file() {
    let e = env("orig\n");
    let mut a = terminal(&e, "a", CFG);
    a.wait_for("opened", |s| has(s, "orig"));
    a.send("X");
    a.send(CTRL_S);
    a.wait_for("saved", |s| has(s, "written"));
    a.sigkill();
    std::fs::write(&e.file, "changed by another editor\n").unwrap();
    let mut b = terminal(&e, "b", CFG);
    b.wait_for("opened", |s| has(s, "another editor"));
    command(&mut b, "hardundo");
    b.wait_for("flagged", |s| has(s, "⚠") && has(s, "second question"));
    b.send("y");
    b.wait_for("the second question", |s| has(s, "overwrite these files as well?"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "changed by another editor\n", "nothing is overwritten before the second yes");
    b.send("y");
    b.wait_for("restored", |s| has(s, "1 file restored"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "orig\n");
}

#[test]
fn the_history_lives_in_the_state_folder_privately_and_off_means_off() {
    use std::os::unix::fs::PermissionsExt;
    let e = env("x\n");
    let mut a = terminal(&e, "a", CFG);
    a.wait_for("opened", |s| has(s, "x"));
    a.send("Y");
    a.send(CTRL_S);
    a.wait_for("saved", |s| has(s, "written"));
    let root: &Path = &e.dir.path().join("state/kuro/history");
    assert!(root.join("files").is_dir() && root.join("blobs").is_dir(), "the history is under $XDG_STATE_HOME/kuro/history");
    assert_eq!(std::fs::metadata(root).unwrap().permissions().mode() & 0o077, 0, "private");
    a.send(CTRL_Q);
    a.send("n");
    // history.enabled = false: nothing is recorded, hard undo says so
    let off = env("x\n");
    let mut b = terminal(&off, "b", &format!("{CFG}[history]\nenabled = false\n"));
    b.wait_for("opened", |s| has(s, "x"));
    b.send("Y");
    b.send(CTRL_S);
    b.wait_for("saved", |s| has(s, "written"));
    assert!(!off.dir.path().join("state/kuro/history/files").exists(), "nothing was recorded");
    command(&mut b, "hardundo");
    b.wait_for("off", |s| has(s, "hard undo is off"));
}

#[test]
fn a_large_history_is_announced_once_and_nothing_is_deleted() {
    let e = env("x\n");
    // a history already bigger than the (tiny) warning size
    let store = kuro::history::Store::new(e.dir.path().join("state/kuro/history"), kuro::history::HistoryOptions::default());
    let p = e.dir.path().join("old.txt");
    std::fs::write(&p, vec![b'o'; 3000]).unwrap();
    store.before_write(&p, Some(&std::fs::read(&p).unwrap()), 5).unwrap();
    store.after_write(&p, None, b"short");
    let cfg = format!("{CFG}[history]\nwarn_mib = 1\n");
    // 1 MiB is not exceeded by a 3 KB history: no message
    let a = terminal(&e, "a", &cfg);
    let s = a.wait_for("opened", |s| has(s, "x"));
    assert!(!has(&s, "hard-undo history uses"), "{}", s.join("\n"));
    drop(a);
    assert!(store.entry_of(&p).is_some());
}

#[test]
fn hard_undo_discards_unsaved_edits_in_every_real_terminal_and_writes_nothing() {
    let e = env("the original text\n");
    let mut a = terminal(&e, "a", CFG);
    a.wait_for("opened", |s| has(s, "the original text"));
    let mut b = terminal(&e, "b", CFG);
    b.wait_for("opened", |s| has(s, "the original text"));
    a.send("UNSAVED ");
    b.wait_for("the edit arrives", |s| has(s, "UNSAVED the original text"));
    command(&mut b, "hardundo");
    b.wait_for("the question", |s| has(s, "discard all unsaved edits"));
    b.send("y");
    b.wait_for("discarded", |s| has(s, "the original text") && !has(s, "UNSAVED") && !has(s, "discard all unsaved"));
    a.wait_for("the other terminal follows", |s| has(s, "the original text") && !has(s, "UNSAVED"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "the original text\n");
    b.assert_default_background();
    b.assert_ansi_palette_only();
}
