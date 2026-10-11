//! Live sync over real Unix sockets and a real file lock: several terminals in one process, a real file,
//! real journal files. The decisions are the same state machines the simulator tests; this checks the plumbing.

use kuro::sync::net::{StartError, SyncEvent, SyncHandle, SyncOptions, key_for};
use kuro::sync::node::State;
use kuro::sync::ot::{self};
use kuro::sync::replica::Effect;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Env {
    _dir: tempfile::TempDir,
    runtime: PathBuf,
    state: PathBuf,
    file: PathBuf,
}

fn env(initial: &str) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("run");
    let state = dir.path().join("state");
    let file = dir.path().join("work.txt");
    std::fs::write(&file, initial).unwrap();
    Env { _dir: dir, runtime, state, file }
}

struct Term {
    h: Option<SyncHandle>,
    text: String,
    notices: Vec<String>,
    saved: usize,
    refused: Vec<String>,
}

impl Term {
    /// Open the first terminal of a session (nobody else exists to answer, so nothing needs pumping).
    fn open(e: &Env, id: u64) -> Result<Term, StartError> {
        Term::open_file(e, &e.file, id, 5 << 20, &mut [])
    }

    /// Open a terminal while `peers` keep running: in real life they are other processes with their own event
    /// loops, so here they are pumped while the newcomer waits for its welcome.
    fn open_with(e: &Env, id: u64, peers: &mut [&mut Term]) -> Result<Term, StartError> {
        Term::open_file(e, &e.file, id, 5 << 20, peers)
    }

    fn open_file(e: &Env, file: &Path, id: u64, max: u64, peers: &mut [&mut Term]) -> Result<Term, StartError> {
        let opts = SyncOptions { runtime_dir: e.runtime.clone(), state_dir: e.state.clone(), max_file_bytes: max, client_id: Some(id) };
        let file = file.to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(SyncHandle::start(&file, opts, Arc::new(|| {})));
        });
        let t = Instant::now();
        let h = loop {
            for p in peers.iter_mut() {
                if p.h.is_some() {
                    p.pump();
                }
            }
            match rx.try_recv() {
                Ok(r) => break r?,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(_) => return Err(StartError::NoLeader),
            }
            assert!(t.elapsed() < Duration::from_secs(15), "joining took too long");
            std::thread::yield_now();
        };
        let text = h.initial_text.clone();
        Ok(Term { h: Some(h), text, notices: vec![], saved: 0, refused: vec![] })
    }

    fn h(&mut self) -> &mut SyncHandle {
        self.h.as_mut().unwrap()
    }

    fn pump(&mut self) {
        let events = self.h().poll();
        for ev in events {
            match ev {
                SyncEvent::Editor(Effect::Replace(t)) => self.text = t,
                SyncEvent::Editor(Effect::Apply { ops, .. }) => ot::apply_all(&mut self.text, &ops),
                SyncEvent::Editor(Effect::Saved { .. }) => self.saved += 1,
                SyncEvent::Editor(Effect::SaveRefused(r) | Effect::SaveFailed(r) | Effect::Conflict(r)) => self.refused.push(r),
                SyncEvent::Editor(Effect::Notice(n)) | SyncEvent::Notice(n) => self.notices.push(n),
                _ => {}
            }
        }
    }

    fn insert(&mut self, pos: usize, s: &str) {
        let op = ot::ins(pos.min(self.text.chars().count()), s);
        ot::apply_str(&mut self.text, &op);
        self.h().local(vec![op]);
    }

    fn is_leader(&self) -> bool {
        self.h.as_ref().unwrap().is_leader()
    }

    fn delete(&mut self, pos: usize, n: usize) {
        let removed: String = self.text.chars().skip(pos).take(n).collect();
        if removed.is_empty() {
            return;
        }
        let op = ot::del(pos, &removed);
        ot::apply_str(&mut self.text, &op);
        self.h().local(vec![op]);
    }

    fn crash(&mut self) {
        self.h.take().unwrap().crash();
    }

    fn quit(&mut self) {
        self.h.take().unwrap().shutdown();
    }
}

/// Open terminals 1..N one after another (each joins while the earlier ones are pumped).
fn open_n<const N: usize>(e: &Env, ids: [u64; N]) -> [Term; N] {
    let mut v: Vec<Term> = vec![];
    for id in ids {
        let t = {
            let mut peers: Vec<&mut Term> = v.iter_mut().collect();
            Term::open_with(e, id, &mut peers).unwrap()
        };
        v.push(t);
    }
    v.try_into().ok().unwrap()
}

/// Pump every terminal until `cond` holds (bounded).
fn settle(terms: &mut [&mut Term], what: &str, cond: impl Fn(&[&mut Term]) -> bool) {
    let t = Instant::now();
    loop {
        for x in terms.iter_mut() {
            if x.h.is_some() {
                x.pump();
            }
        }
        if cond(terms) {
            return;
        }
        assert!(
            t.elapsed() < Duration::from_secs(15),
            "timed out waiting for: {what}\ntexts: {:?}",
            terms.iter().map(|t| t.text.clone()).collect::<Vec<_>>()
        );
        std::thread::yield_now();
    }
}

fn same_text(ts: &[&mut Term], want: &str) -> bool {
    ts.iter().all(|t| t.text == want)
}

#[test]
fn the_first_terminal_leads_and_later_ones_follow() {
    let e = env("hello\n");
    let [a, b] = open_n(&e, [1, 2]);
    assert!(a.is_leader() && !b.is_leader());
    assert_eq!(b.text, "hello\n");
}

#[test]
fn edits_flow_between_terminals_without_saving() {
    let e = env("hello\n");
    let [mut a, mut b, mut c] = open_n(&e, [1, 2, 3]);
    a.insert(0, ">> ");
    b.insert(8, "world\n");
    settle(&mut [&mut a, &mut b, &mut c], "all three show both edits", |t| same_text(t, ">> hello\nworld\n"));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "hello\n", "nothing was saved");
}

#[test]
fn edit_in_two_terminals_and_save_from_a_third_writes_everything() {
    let e = env("0123456789");
    let [mut a, mut b, mut c] = open_n(&e, [1, 2, 3]);
    a.insert(0, "A");
    b.insert(5, "B");
    settle(&mut [&mut a, &mut b, &mut c], "edits arrived", |t| same_text(t, "A01234B56789"));
    c.h().save(false);
    settle(&mut [&mut a, &mut b, &mut c], "saved everywhere", |t| t.iter().all(|x| x.saved == 1));
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "A01234B56789");
    // no temporary files left behind
    let leftovers: Vec<_> = std::fs::read_dir(e.file.parent().unwrap())
        .unwrap()
        .map(|x| x.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("kuro-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn undo_in_another_terminal_undoes_the_last_edit() {
    let e = env("");
    let [mut a, mut b] = open_n(&e, [1, 2]);
    a.insert(0, "typed in a");
    settle(&mut [&mut a, &mut b], "b sees it", |t| same_text(t, "typed in a"));
    b.h().undo();
    settle(&mut [&mut a, &mut b], "undone everywhere", |t| same_text(t, ""));
}

#[test]
fn killing_the_leader_hands_over_and_editing_continues() {
    let e = env("base\n");
    let [mut a, mut b, mut c] = open_n(&e, [1, 2, 3]);
    b.insert(5, "from b\n");
    settle(&mut [&mut a, &mut b, &mut c], "synced", |t| same_text(t, "base\nfrom b\n"));
    a.crash();
    // a new leader emerges among b and c, and they keep syncing
    c.insert(0, "C:");
    settle(&mut [&mut b, &mut c], "synced after the handover", |t| same_text(t, "C:base\nfrom b\n"));
    assert!(b.is_leader() ^ c.is_leader(), "exactly one of the survivors leads");
    b.insert(0, "B:");
    settle(&mut [&mut b, &mut c], "still syncing", |t| same_text(t, "B:C:base\nfrom b\n") || same_text(t, "C:B:base\nfrom b\n"));
}

#[test]
fn a_save_after_the_handover_works() {
    let e = env("x");
    let [mut a, mut b] = open_n(&e, [1, 2]);
    b.insert(1, "yz");
    settle(&mut [&mut a, &mut b], "synced", |t| same_text(t, "xyz"));
    a.crash();
    b.h().save(false);
    settle(&mut [&mut b], "saved", |t| t[0].saved == 1);
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "xyz");
}

#[test]
fn cascading_crashes_still_leave_the_survivors_in_agreement() {
    let e = env("start\n");
    let [mut a, mut b, mut c, mut d] = open_n(&e, [1, 2, 3, 4]);
    c.insert(0, "c1 ");
    d.insert(0, "d1 ");
    a.crash();
    b.crash();
    c.insert(0, "c2 ");
    settle(&mut [&mut c, &mut d], "the last two agree", |t| {
        t[0].text == t[1].text && t[0].text.contains("c1") && t[0].text.contains("d1") && t[0].text.contains("c2")
    });
}

#[test]
fn a_new_terminal_can_join_after_a_handover() {
    let e = env("seed");
    let [mut a, mut b] = open_n(&e, [1, 2]);
    b.insert(4, "!");
    settle(&mut [&mut a, &mut b], "synced", |t| same_text(t, "seed!"));
    a.crash();
    settle(&mut [&mut b], "b leads", |t| t[0].is_leader());
    let mut c = Term::open_with(&e, 3, &mut [&mut b]).unwrap();
    assert_eq!(c.text, "seed!");
    c.insert(0, ">");
    settle(&mut [&mut b, &mut c], "synced", |t| same_text(t, ">seed!"));
}

#[test]
fn a_stale_socket_and_lock_from_a_crashed_leader_do_not_block_a_new_session() {
    let e = env("old\n");
    let mut a = Term::open(&e, 1).unwrap();
    a.crash();
    // everything died: the socket file may still be on disk, the lock is gone
    let mut b = Term::open(&e, 2).unwrap();
    assert!(b.is_leader());
    b.insert(0, "new ");
    settle(&mut [&mut b], "own edit applied", |t| t[0].text == "new old\n");
}

#[test]
fn sockets_and_runtime_directory_are_private() {
    let e = env("x");
    let _a = Term::open(&e, 1).unwrap();
    let key = key_for(&std::fs::canonicalize(&e.file).unwrap());
    assert_eq!(std::fs::metadata(&e.runtime).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(e.runtime.join(format!("{key}.sock"))).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn different_files_do_not_interfere() {
    let e = env("one");
    let other = e.file.parent().unwrap().join("other.txt");
    std::fs::write(&other, "two").unwrap();
    let mut a = Term::open(&e, 1).unwrap();
    let mut b = Term::open_file(&e, &other, 2, 5 << 20, &mut []).unwrap();
    assert!(a.is_leader() && b.is_leader(), "each file has its own leader");
    a.insert(3, "!");
    settle(&mut [&mut a, &mut b], "a edited", |t| t[0].text == "one!");
    assert_eq!(b.text, "two");
}

#[test]
fn files_over_the_limit_are_not_synced_and_the_error_says_how_big() {
    let e = env(&"x".repeat(3000));
    match Term::open_file(&e, &e.file, 1, 2048, &mut []) {
        Err(StartError::TooLarge(n)) => assert_eq!(n, 3000),
        other => panic!("expected TooLarge, got {:?}", other.err()),
    }
    assert!(Term::open_file(&e, &e.file, 1, 3000, &mut []).is_ok(), "exactly at the limit still syncs");
}

#[test]
fn a_terminal_joining_a_session_that_has_grown_past_the_limit_is_refused() {
    let e = env("small");
    let mut a = Term::open_file(&e, &e.file, 1, 100, &mut []).unwrap();
    a.insert(5, &"y".repeat(500));
    settle(&mut [&mut a], "grown", |t| t[0].text.len() > 400);
    // the leader's text is now above the limit; a new terminal must not join (the sync continues for existing ones)
    // give the leader a moment to commit
    let t = Instant::now();
    loop {
        match Term::open_file(&e, &e.file, 2, 100, &mut [&mut a]) {
            Err(StartError::TooLarge(n)) => {
                assert!(n > 100);
                break;
            }
            Ok(mut b) => {
                b.quit();
                assert!(t.elapsed() < Duration::from_secs(10), "the leader never reported the larger text");
            }
            Err(other) => panic!("{other}"),
        }
    }
    a.insert(0, "still ");
    settle(&mut [&mut a], "the existing terminal keeps working", |t| t[0].text.starts_with("still "));
}

#[test]
fn binary_files_are_refused_rather_than_corrupted() {
    let e = env("");
    std::fs::write(&e.file, [0xff, 0xfe, 0x00, 0x41]).unwrap();
    assert!(matches!(Term::open(&e, 1), Err(StartError::NotUtf8)));
    assert_eq!(std::fs::read(&e.file).unwrap(), vec![0xff, 0xfe, 0x00, 0x41], "and the file is untouched");
}

#[test]
fn an_external_change_reaches_every_terminal() {
    let e = env("one\ntwo\n");
    let [mut a, mut b] = open_n(&e, [1, 2]);
    std::fs::write(&e.file, "one\n2\n").unwrap();
    settle(&mut [&mut a, &mut b], "reloaded", |t| same_text(t, "one\n2\n"));
}

#[test]
fn unsaved_edits_survive_every_process_dying_via_the_journal() {
    let e = env("base\n");
    let [mut a, mut b] = open_n(&e, [1, 2]);
    a.insert(5, "recovered work\n");
    settle(&mut [&mut a, &mut b], "synced", |t| same_text(t, "base\nrecovered work\n"));
    // wait until the journal is durable (it is flushed about once a second)
    let key = key_for(&std::fs::canonicalize(&e.file).unwrap());
    let jp = e.state.join("recovery").join(format!("{key}.journal"));
    let t = Instant::now();
    while !std::fs::read_to_string(&jp).unwrap_or_default().contains("recovered work") {
        a.pump(); // the leader flushes its journal from its regular polling
        b.pump();
        assert!(t.elapsed() < Duration::from_secs(10), "the journal was never flushed");
        std::thread::yield_now();
    }
    a.crash();
    b.crash();
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "base\n", "nothing was ever saved");
    let mut c = Term::open(&e, 3).unwrap();
    assert_eq!(c.text, "base\nrecovered work\n", "the unsaved work came back");
    settle(&mut [&mut c], "told about the recovery", |t| t[0].notices.iter().any(|n| n.contains("recovered")));
    c.h().save(false);
    settle(&mut [&mut c], "saved", |t| t[0].saved == 1);
    assert_eq!(std::fs::read_to_string(&e.file).unwrap(), "base\nrecovered work\n");
    // after the save the journal no longer holds the edits
    assert!(!std::fs::read_to_string(&jp).unwrap_or_default().contains("recovered work"));
}

#[test]
fn a_clean_quit_of_the_leader_flushes_and_hands_over() {
    let e = env("q\n");
    let [mut a, mut b] = open_n(&e, [1, 2]);
    a.insert(2, "typed\n");
    settle(&mut [&mut a, &mut b], "synced", |t| same_text(t, "q\ntyped\n"));
    a.quit();
    b.insert(0, ">");
    settle(&mut [&mut b], "b carries on as leader", |t| t[0].text == ">q\ntyped\n" && t[0].is_leader());
    assert_eq!(b.h().state(), State::Live);
}

#[test]
fn deleting_and_inserting_concurrently_in_real_time_converges() {
    let e = env("the quick brown fox jumps over the lazy dog");
    let [mut a, mut b] = open_n(&e, [1, 2]);
    for i in 0..40 {
        a.insert(0, &format!("a{i} "));
        b.delete(10, 2);
        b.insert(12, &format!("b{i} "));
        a.pump();
        b.pump();
    }
    settle(&mut [&mut a, &mut b], "converged", |t| t[0].text == t[1].text);
    // every character that was typed and not deleted is accounted for: 40 a-tokens and 40 b-tokens add a fixed
    // number of characters; deletions can only have removed original text or typed text
    let a_chars: usize = (0..40).map(|i| format!("a{i} ").chars().count()).sum();
    let b_chars: usize = (0..40).map(|i| format!("b{i} ").chars().count()).sum();
    let original = "the quick brown fox jumps over the lazy dog".chars().count();
    let len = a.text.chars().count();
    assert!(len <= original + a_chars + b_chars, "no text appeared from nowhere: {len}");
    assert!(len >= original + a_chars + b_chars - 80, "at most 40 deletes of 2 characters removed text: {len}");
}
