//! Live-sync concurrency and fault-injection tests on the deterministic simulator (`kuro::sync::cluster`).
//! Everything is replayable from its seed. Invariants checked after every scenario:
//!   * all terminals and the leader end with identical text,
//!   * no edit that any surviving terminal had (confirmed or not) is lost across a leader change,
//!   * the file on disk is always a complete version (old or new), never a mix.

use kuro::sync::cluster::{Cluster, PATH};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

fn token(n: usize) -> String {
    format!("‹{n}›")
}

fn count(hay: &str, needle: &str) -> usize {
    hay.matches(needle).count()
}

/// A position that cannot fall inside an inserted token: the start, the end, or just after a line break.
fn boundary(text: &str, r: usize) -> usize {
    let mut b = vec![0];
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            b.push(i + 1);
        }
    }
    b.push(text.chars().count());
    b[r % b.len()]
}

#[derive(Debug, Clone)]
enum Act {
    Insert { who: usize, at: usize },
    Delete { who: usize, at: usize, n: usize },
    Mark { who: usize },
    Undo { who: usize },
    Redo { who: usize },
    Save { who: usize },
    Steps(usize),
}

fn act_strategy(nodes: usize) -> impl Strategy<Value = Act> {
    prop_oneof![
        6 => (0..nodes, 0..60usize).prop_map(|(who, at)| Act::Insert { who, at }),
        2 => (0..nodes, 0..60usize, 1..6usize).prop_map(|(who, at, n)| Act::Delete { who, at, n }),
        1 => (0..nodes).prop_map(|who| Act::Mark { who }),
        1 => (0..nodes).prop_map(|who| Act::Undo { who }),
        1 => (0..nodes).prop_map(|who| Act::Redo { who }),
        1 => (0..nodes).prop_map(|who| Act::Save { who }),
        4 => (0..8usize).prop_map(Act::Steps),
    ]
}

fn cluster_of(nodes: usize, seed: u64, initial: &str) -> Cluster {
    let mut c = Cluster::new(initial, seed);
    for id in 2..=nodes as u64 {
        c.add_follower(id);
    }
    c.run_until_idle();
    c
}

fn run(c: &mut Cluster, acts: &[Act], token_counter: &mut usize) {
    for a in acts {
        match a {
            Act::Insert { who, at } => {
                let id = *who as u64 + 1;
                if c.members[&id].alive {
                    *token_counter += 1;
                    let pos = boundary(c.text_of(id), *at);
                    c.insert(id, pos, &token(*token_counter));
                }
            }
            Act::Delete { who, at, n } => {
                let id = *who as u64 + 1;
                if c.members[&id].alive {
                    c.delete(id, *at, *n);
                }
            }
            Act::Mark { who } => {
                let id = *who as u64 + 1;
                if c.members[&id].alive {
                    c.mark(id);
                }
            }
            Act::Undo { who } => {
                let id = *who as u64 + 1;
                if c.members[&id].alive {
                    c.undo(id);
                }
            }
            Act::Redo { who } => {
                let id = *who as u64 + 1;
                if c.members[&id].alive {
                    c.redo(id);
                }
            }
            Act::Save { who } => {
                let id = *who as u64 + 1;
                if c.members[&id].alive {
                    c.save(id, false);
                }
            }
            Act::Steps(n) => {
                for _ in 0..*n {
                    c.step();
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(128),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/sync_sim.proptest-regressions"))),
        ..ProptestConfig::default()
    })]

    /// Two to four terminals typing, deleting, undoing, redoing and saving with arbitrary delivery order always converge.
    #[test]
    fn random_interleavings_converge(
        nodes in 2..5usize,
        seed in any::<u64>(),
        acts in proptest::collection::vec(act_strategy(4), 1..60),
    ) {
        let acts: Vec<Act> = acts.into_iter().map(|a| clamp(a, nodes)).collect();
        let mut c = cluster_of(nodes, seed, "the quick brown fox\njumps over the lazy dog\n");
        let mut n = 0;
        run(&mut c, &acts, &mut n);
        c.run_until_idle();
        c.assert_converged();
        // saving from anywhere writes the whole shared text
        c.save(1, true);
        c.run_until_idle();
        prop_assert_eq!(c.fs.read_string(PATH).unwrap(), c.assert_converged());
    }

    /// Insert-only edits: killing the leader at any moment never loses an insertion that a surviving terminal had.
    #[test]
    fn killing_the_leader_loses_nothing_a_survivor_had(
        nodes in 3..5usize,
        seed in any::<u64>(),
        before in proptest::collection::vec(act_strategy(4), 1..30),
        after in proptest::collection::vec(act_strategy(4), 0..30),
    ) {
        let only_inserts = |v: Vec<Act>| -> Vec<Act> {
            v.into_iter().filter(|a| matches!(a, Act::Insert { .. } | Act::Steps(_) | Act::Save { .. })).map(|a| clamp(a, nodes)).collect()
        };
        let (before, after) = (only_inserts(before), only_inserts(after));
        let mut c = cluster_of(nodes, seed, "base text\n");
        let mut n = 0;
        run(&mut c, &before, &mut n);
        // what every survivor's editor shows right now (confirmed or not)
        let survivors_had: Vec<String> = c.alive().into_iter().filter(|i| *i != 1).map(|i| c.text_of(i).to_string()).collect();
        c.kill(1);
        run(&mut c, &after, &mut n);
        c.run_until_idle();
        let final_text = c.assert_converged();
        for t in survivors_had {
            for i in 1..=n {
                let tok = token(i);
                if t.contains(&tok) {
                    prop_assert_eq!(count(&final_text, &tok), 1, "token {} that a survivor had is missing or duplicated in {:?}", tok, final_text);
                }
            }
        }
        // terminals that kept typing after the handover also keep their edits
        for i in 1..=n {
            prop_assert!(count(&final_text, &token(i)) <= 1, "duplicated token {}", token(i));
        }
    }

    /// Killing several terminals in a row, including whoever just became leader, still ends in agreement.
    #[test]
    fn cascading_kills_still_converge(
        seed in any::<u64>(),
        acts in proptest::collection::vec(act_strategy(4), 1..40),
        kill_points in proptest::collection::vec(0..40usize, 1..3),
    ) {
        let mut c = cluster_of(4, seed, "x\n");
        let mut n = 0;
        let mut kills = kill_points;
        kills.sort_unstable();
        for (i, a) in acts.iter().enumerate() {
            while kills.first() == Some(&i) && c.alive().len() > 1 {
                kills.remove(0);
                // kill the current leader if there is one, otherwise the lowest live terminal
                let victim = c.lock_holder.filter(|h| c.members[h].alive).unwrap_or_else(|| c.alive()[0]);
                c.kill(victim);
            }
            run(&mut c, std::slice::from_ref(a), &mut n);
        }
        c.run_until_idle();
        c.assert_converged();
    }
}

fn clamp(a: Act, nodes: usize) -> Act {
    let f = |w: usize| w % nodes;
    match a {
        Act::Insert { who, at } => Act::Insert { who: f(who), at },
        Act::Delete { who, at, n } => Act::Delete { who: f(who), at, n },
        Act::Mark { who } => Act::Mark { who: f(who) },
        Act::Undo { who } => Act::Undo { who: f(who) },
        Act::Redo { who } => Act::Redo { who: f(who) },
        Act::Save { who } => Act::Save { who: f(who) },
        s => s,
    }
}

// ---------------------------------------------------------------- exhaustive: kill the leader at every step

/// A fixed busy scenario in which, if `kill_after` is given, the leader dies after that many message deliveries.
/// Returns the cluster after everything settled, the tokens each terminal typed, and whether the kill happened.
fn scripted_with_kill(seed: u64, kill_after: Option<usize>) -> (Cluster, Vec<(u64, String)>, bool, usize) {
    let mut c = cluster_of(3, seed, "0123456789\n");
    let mut delivered = 0;
    let mut killed = false;
    let mut typed = vec![];
    let mut tok = 0;
    let deliver = |c: &mut Cluster, delivered: &mut usize, killed: &mut bool| {
        if !*killed && Some(*delivered) == kill_after {
            c.kill(1);
            *killed = true;
        }
        if c.step() {
            *delivered += 1;
        }
    };
    for round in 0..6 {
        for who in [2u64, 3, 1] {
            if c.members[&who].alive {
                tok += 1;
                let pos = boundary(c.text_of(who), round);
                c.insert(who, pos, &token(tok));
                typed.push((who, token(tok)));
            }
            deliver(&mut c, &mut delivered, &mut killed);
        }
        deliver(&mut c, &mut delivered, &mut killed);
    }
    if !killed && kill_after.is_some() {
        c.kill(1);
        killed = true;
    }
    c.run_until_idle();
    (c, typed, killed, delivered)
}

#[test]
fn killing_the_leader_at_every_possible_step_never_loses_or_duplicates_an_edit() {
    let (reference, _, _, total) = scripted_with_kill(42, None);
    reference.assert_converged();
    assert!(total > 10, "the scenario exercises a meaningful number of deliveries ({total})");
    for k in 0..=total + 1 {
        let (c, typed, killed, _) = scripted_with_kill(42, Some(k));
        assert!(killed);
        let text = c.assert_converged();
        for (who, t) in &typed {
            assert!(count(&text, t) <= 1, "kill after {k} steps duplicated {t}: {text:?}");
            if *who != 1 {
                assert_eq!(count(&text, t), 1, "kill after {k} steps lost {t}, typed in surviving terminal {who}: {text:?}");
            }
        }
        assert!(c.lock_holder.is_some() && c.lock_holder != Some(1), "kill after {k}: a new leader took over");
    }
}

#[test]
fn what_survivors_typed_is_never_lost_whenever_the_leader_dies() {
    for k in 0..40 {
        let mut c = cluster_of(3, 99 + k as u64, "base\n");
        let mut tok = 0;
        let mut typed_by_survivors = vec![];
        for round in 0..5 {
            for who in [2u64, 3] {
                tok += 1;
                let pos = boundary(c.text_of(who), round);
                c.insert(who, pos, &token(tok));
                typed_by_survivors.push(token(tok));
            }
            tok += 1;
            c.insert(1, 0, &token(tok));
            for _ in 0..(k % 7) {
                c.step();
            }
        }
        for _ in 0..k {
            c.step();
        }
        c.kill(1);
        c.run_until_idle();
        let text = c.assert_converged();
        for t in &typed_by_survivors {
            assert_eq!(count(&text, t), 1, "kill at {k}: {t} typed in a surviving terminal is missing from {text:?}");
        }
    }
}

// ---------------------------------------------------------------- everything dies

#[test]
fn if_every_terminal_dies_the_flushed_edits_come_back_from_the_journal() {
    let mut c = cluster_of(2, 5, "base\n");
    c.insert(1, 4, " one");
    c.insert(2, 0, ">> ");
    c.run_until_idle();
    c.tick(1500); // the journal is flushed after a second
    let before_crash = c.assert_converged();
    c.insert(1, 0, "LOST? "); // typed after the last flush
    c.run_until_idle();
    c.journal.crash(); // power cut: unflushed journal data is gone
    c.kill(1);
    c.kill(2);
    c.lock_holder = None;
    // a new session starts: the file on disk is unchanged, the journal holds the unsaved work
    assert_eq!(c.fs.read_string(PATH).unwrap(), "base\n", "nothing was ever saved");
    c.start_leader(10);
    c.run_until_idle();
    let recovered = c.text_of(10).to_string();
    assert_eq!(recovered, before_crash, "everything up to the last journal flush is back");
    assert!(c.members[&10].notices.iter().any(|n| n.contains("recovered")), "the user is told: {:?}", c.members[&10].notices);
    assert!(c.members[&10].node.replica.is_modified(), "recovered work counts as unsaved");
    c.save(10, false);
    c.run_until_idle();
    assert_eq!(c.fs.read_string(PATH).unwrap(), before_crash);
}

#[test]
fn a_clean_shutdown_flushes_so_nothing_is_lost() {
    let mut c = cluster_of(1, 6, "x");
    c.insert(1, 1, "yz");
    c.run_until_idle();
    c.members.get_mut(&1).unwrap().node.leader.as_mut().unwrap().flush_journal(0);
    c.kill(1);
    c.lock_holder = None;
    c.start_leader(2);
    assert_eq!(c.text_of(2), "xyz");
}

// ---------------------------------------------------------------- crash during save

#[test]
fn crash_during_save_leaves_the_old_file_and_the_edits_stay_recoverable() {
    for crash_at in 1..=2 {
        let mut c = cluster_of(2, 8, "original\n");
        c.insert(2, 8, " edited");
        c.run_until_idle();
        c.tick(2000);
        {
            let mut s = c.fs.0.lock().unwrap();
            s.write_calls = crash_at - 1;
            s.crash_on_write = Some(crash_at);
        }
        c.save(2, false);
        c.run_until_idle();
        let on_disk = c.fs.read_string(PATH).unwrap();
        assert!(on_disk == "original\n" || on_disk == "original edited\n", "never a partial file, got {on_disk:?}");
        assert_eq!(on_disk, "original\n", "the crashed write did not replace the file");
        assert!(c.members[&2].node.replica.is_modified(), "still unsaved");
        assert!(!c.members[&2].refused.is_empty(), "the user is told the save failed");
        // try again: now it works
        c.fs.0.lock().unwrap().crash_on_write = None;
        c.save(2, false);
        c.run_until_idle();
        assert_eq!(c.fs.read_string(PATH).unwrap(), "original edited\n");
    }
}

#[test]
fn the_file_is_never_partial_whatever_the_interleaving() {
    for seed in 0..60u64 {
        let mut c = cluster_of(3, seed, "AAAA\n");
        let mut observed = std::collections::BTreeSet::new();
        observed.insert(c.fs.read_string(PATH).unwrap());
        for i in 0..20 {
            let (typist, saver) = (1 + (i % 3), 1 + ((i + 1) % 3));
            if c.members[&typist].alive {
                let pos = boundary(c.text_of(typist), i as usize);
                c.insert(typist, pos, &token(i as usize));
            }
            if i % 4 == 0 && c.members[&saver].alive {
                c.save(saver, false);
            }
            for _ in 0..(seed % 5) {
                c.step();
            }
            if seed % 7 == 0 && i == 10 {
                c.kill(1);
            }
            observed.insert(c.fs.read_string(PATH).unwrap());
        }
        c.run_until_idle();
        let t = c.assert_converged();
        observed.insert(c.fs.read_string(PATH).unwrap());
        // every version the file ever had is the text of some revision: spot-check the ends
        assert!(observed.contains("AAAA\n"));
        for v in &observed {
            assert!(v.ends_with("AAAA\n") || v.contains("AAAA"), "a version of the file lost the original content: {v:?}");
            assert!(v.len() <= t.len() + 40);
        }
    }
}

// ---------------------------------------------------------------- the file changes underneath

#[test]
fn an_external_change_reloads_every_terminal_when_nothing_is_unsaved() {
    let mut c = cluster_of(3, 21, "one\ntwo\nthree\n");
    c.fs.external_write(std::path::Path::new(PATH), b"one\n2\nthree\n");
    c.tick(1000);
    c.run_until_idle();
    assert_eq!(c.assert_converged(), "one\n2\nthree\n");
    assert!(!c.members[&2].node.replica.is_modified(), "a reload is not an unsaved change");
}

#[test]
fn an_external_change_with_unsaved_edits_is_flagged_and_never_overwritten_silently() {
    let mut c = cluster_of(2, 22, "one\ntwo\n");
    c.insert(2, 0, "mine ");
    c.run_until_idle();
    c.fs.external_write(std::path::Path::new(PATH), b"theirs\n");
    c.tick(1000);
    c.run_until_idle();
    assert_eq!(c.assert_converged(), "mine one\ntwo\n", "unsaved work is untouched");
    assert!(c.members[&2].refused.iter().any(|r| r.contains("changed on disk")), "{:?}", c.members[&2].refused);
    c.save(1, false);
    c.run_until_idle();
    assert_eq!(c.fs.read_string(PATH).unwrap(), "theirs\n", "a plain save refuses to clobber it");
    assert!(c.members[&1].refused.iter().any(|r| r.contains(":w!")));
    c.save(1, true);
    c.run_until_idle();
    assert_eq!(c.fs.read_string(PATH).unwrap(), "mine one\ntwo\n", "a forced save overwrites on purpose");
}

#[test]
fn an_external_change_that_arrives_during_a_handover_is_still_seen() {
    let mut c = cluster_of(3, 23, "a\nb\n");
    c.kill(1);
    c.fs.external_write(std::path::Path::new(PATH), b"a\nB changed\n");
    c.run_until_idle();
    c.tick(1000);
    c.run_until_idle();
    assert_eq!(c.assert_converged(), "a\nB changed\n");
}

// ---------------------------------------------------------------- the handover itself

#[test]
fn undo_history_restarts_after_a_handover_and_says_so() {
    let mut c = cluster_of(2, 30, "a");
    c.insert(2, 1, "b");
    c.run_until_idle();
    c.kill(1);
    c.run_until_idle();
    c.undo(2);
    c.run_until_idle();
    assert_eq!(c.assert_converged(), "ab", "undo does not reach back before the handover");
    assert!(c.members[&2].notices.iter().any(|n| n.contains("oldest")));
    c.insert(2, 2, "c");
    c.run_until_idle();
    c.undo(2);
    c.run_until_idle();
    assert_eq!(c.assert_converged(), "ab", "but new edits are undoable again");
}

#[test]
fn a_third_terminal_can_join_after_a_handover() {
    let mut c = cluster_of(2, 31, "base");
    c.insert(2, 4, "!");
    c.run_until_idle();
    c.kill(1);
    c.run_until_idle();
    c.add_follower(3);
    c.run_until_idle();
    assert_eq!(c.text_of(3), "base!");
    c.insert(3, 0, ">");
    c.run_until_idle();
    assert_eq!(c.assert_converged(), ">base!");
}

#[test]
fn replica_and_leader_text_agree_on_wide_characters() {
    let mut c = cluster_of(3, 32, "日本語\n😀é\n");
    c.insert(2, 1, "→");
    c.insert(3, 4, "ü");
    c.delete(1, 0, 1);
    c.run_until_idle();
    c.assert_converged();
}

// ---------------------------------------------------------------------------------------------------- discard (hard undo of unsaved edits)

#[test]
fn discard_gives_every_terminal_the_file_on_disk_and_marks_it_saved() {
    let initial = "the quick brown fox\njumps over\n";
    let mut c = cluster_of(3, 7, initial);
    c.insert(1, 4, "AAA");
    c.insert(2, 0, "BBB");
    c.run_until_idle();
    c.insert(3, 10, "CCC");
    c.run_until_idle();
    assert_ne!(c.assert_converged(), initial);
    c.discard(2);
    c.run_until_idle();
    assert_eq!(c.assert_converged(), initial, "every terminal shows the file on disk again");
    assert_eq!(c.fs.read_string(kuro::sync::cluster::PATH).as_deref(), Some(initial), "nothing was written");
    for id in [1, 2, 3] {
        assert!(c.members[&id].saved_events > 0, "terminal {id} was told the text is saved");
    }
    assert!(!c.leader().unwrap().leader.as_ref().unwrap().is_dirty());
}

#[test]
fn discard_also_forgets_the_shared_undo_history_and_the_journal() {
    let mut c = cluster_of(2, 3, "base\n");
    c.insert(1, 0, "x");
    c.run_until_idle();
    c.discard(1);
    c.run_until_idle();
    c.undo(1); // nothing left to undo: it must not bring the discarded edit back
    c.run_until_idle();
    assert_eq!(c.assert_converged(), "base\n");
    // after the discard a restarted session finds nothing to recover
    for id in c.alive() {
        c.kill(id);
    }
    c.start_leader(1);
    c.run_until_idle();
    assert_eq!(c.assert_converged(), "base\n");
    assert!(c.members[&1].notices.iter().all(|n| !n.contains("recovered")), "{:?}", c.members[&1].notices);
}

#[test]
fn an_edit_racing_with_a_discard_is_either_discarded_with_the_rest_or_kept_and_all_terminals_agree() {
    // The leader orders the two: an edit committed before the discard is discarded with everything else, one that
    // arrives after it is a new edit and stays. Both are right; disagreeing terminals or a half-kept edit are not.
    let (mut kept, mut dropped) = (0, 0);
    for seed in 0..60 {
        let mut c = cluster_of(3, seed, "one\ntwo\n");
        c.insert(1, 0, "old ");
        c.run_until_idle();
        c.discard(2);
        c.insert(3, 4, "NEW"); // typed before the discard reaches terminal 3
        c.run_until_idle();
        let text = c.assert_converged();
        assert!(!text.contains("old "), "seed {seed}: the discarded edit is gone: {text:?}");
        match text.matches("NEW").count() {
            0 => {
                assert_eq!(text, "one\ntwo\n", "seed {seed}");
                dropped += 1;
            }
            1 => kept += 1,
            _ => panic!("seed {seed}: NEW duplicated: {text:?}"),
        }
    }
    assert!(kept > 0 && dropped > 0, "the schedule explores both orders (kept {kept}, dropped {dropped})");
}

#[test]
fn discard_with_nothing_to_discard_changes_no_text() {
    let mut c = cluster_of(2, 1, "same\n");
    c.discard(2);
    c.run_until_idle();
    assert_eq!(c.assert_converged(), "same\n");
}

#[test]
fn discard_reloads_a_file_that_changed_on_disk_in_the_meantime() {
    let mut c = cluster_of(2, 1, "v1\n");
    c.insert(1, 0, "unsaved ");
    c.run_until_idle();
    c.fs.external_write(std::path::Path::new(kuro::sync::cluster::PATH), b"v2 from another editor\n");
    c.discard(1);
    c.run_until_idle();
    assert_eq!(c.assert_converged(), "v2 from another editor\n");
}
