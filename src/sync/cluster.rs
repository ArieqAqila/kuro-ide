//! A deterministic simulator of several terminals and a leader, for tests: message queues with controllable
//! delivery order, terminals that can be killed at any moment, a simulated OS lock that is released when its
//! holder dies, and checks that everyone ends up identical.
//!
//! Nothing here uses threads, sockets or the clock: the same `Node` state machines the real runtime drives
//! are driven here by explicit steps, so a failing scenario replays exactly from its seed.

use super::node::{Input, Node, Output};
use super::ot::{self, Op};
use super::protocol::*;
use super::replica::Effect;
use super::sim::{SimFs, SimJournal};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

pub const PATH: &str = "/work/shared.txt";

#[derive(Debug, Clone)]
enum Wire {
    ToLeader(ClientMsg),
    ToClient(ServerMsg),
    /// The other end closed the connection (arrives after everything it sent before).
    Eof,
}

pub struct Member {
    pub node: Node,
    /// What this terminal's editor shows.
    pub text: String,
    pub alive: bool,
    connected_to: Option<ClientId>,
    pub notices: Vec<String>,
    pub saved_events: usize,
    pub refused: Vec<String>,
}

pub struct Cluster {
    pub fs: SimFs,
    pub journal: SimJournal,
    pub members: BTreeMap<ClientId, Member>,
    queues: BTreeMap<(ClientId, ClientId), VecDeque<Wire>>,
    /// Who holds the (simulated) leader lock.
    pub lock_holder: Option<ClientId>,
    pub time_ms: u64,
    rng: u64,
    pub log: Vec<String>,
}

fn path() -> PathBuf {
    PathBuf::from(PATH)
}

impl Cluster {
    /// A cluster with one terminal (id 1), the leader, editing a file that holds `initial`.
    pub fn new(initial: &str, seed: u64) -> Cluster {
        let fs = SimFs::with_file(PATH, initial);
        let journal = SimJournal::default();
        let mut c = Cluster {
            fs,
            journal,
            members: BTreeMap::new(),
            queues: BTreeMap::new(),
            lock_holder: None,
            time_ms: 0,
            rng: seed.wrapping_mul(0x9E3779B97F4A7C15) | 1,
            log: vec![],
        };
        c.start_leader(1);
        c
    }

    fn factories(&self) -> (super::node::FsFactory, super::node::JournalFactory) {
        let (fs, j) = (self.fs.clone(), self.journal.clone());
        (Box::new(move || Box::new(fs.clone())), Box::new(move || Box::new(j.clone())))
    }

    /// Start a brand-new session's leader (also used to restart after everything died).
    pub fn start_leader(&mut self, id: ClientId) {
        let (ff, jf) = self.factories();
        let (node, outs) = Node::new_leader(id, path(), ff, jf, self.time_ms).expect("leader opens");
        self.members.insert(
            id,
            Member { node, text: String::new(), alive: true, connected_to: None, notices: vec![], saved_events: 0, refused: vec![] },
        );
        self.lock_holder = Some(id);
        self.process(id, outs);
    }

    pub fn add_follower(&mut self, id: ClientId) {
        let (ff, jf) = self.factories();
        let node = Node::follower(id, path(), ff, jf);
        let leader = self.lock_holder.expect("a leader exists");
        self.members.insert(
            id,
            Member {
                node,
                text: String::new(),
                alive: true,
                connected_to: Some(leader),
                notices: vec![],
                saved_events: 0,
                refused: vec![],
            },
        );
        let outs = self.members.get_mut(&id).unwrap().node.input(Input::Connected);
        self.process(id, outs);
    }

    fn rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    pub fn rand_below(&mut self, n: usize) -> usize {
        (self.rand() % n.max(1) as u64) as usize
    }

    fn push(&mut self, from: ClientId, to: ClientId, w: Wire) {
        self.queues.entry((from, to)).or_default().push_back(w);
    }

    /// Carry out what `id`'s node asked for.
    fn process(&mut self, id: ClientId, outs: Vec<Output>) {
        let mut work: VecDeque<(ClientId, Output)> = outs.into_iter().map(|o| (id, o)).collect();
        while let Some((who, o)) = work.pop_front() {
            match o {
                Output::ToLeader(m) => {
                    if let Some(l) = self.members[&who].connected_to {
                        self.push(who, l, Wire::ToLeader(m));
                    }
                }
                Output::ToClient(c, m) => self.push(who, c, Wire::ToClient(m)),
                Output::Notice(n) => {
                    self.log.push(format!("{who}: {n}"));
                    self.members.get_mut(&who).unwrap().notices.push(n);
                }
                Output::Editor(e) => {
                    let m = self.members.get_mut(&who).unwrap();
                    match e {
                        Effect::Replace(t) => m.text = t,
                        Effect::Apply { ops, .. } => ot::apply_all(&mut m.text, &ops),
                        Effect::Saved { .. } => m.saved_events += 1,
                        Effect::SaveRefused(r) | Effect::SaveFailed(r) | Effect::Conflict(r) => m.refused.push(r),
                        Effect::Notice(n) => m.notices.push(n),
                        Effect::Reloaded | Effect::Send(_) | Effect::NeedResync => {}
                    }
                }
                Output::TryElection => {
                    // the simulated flock: the first terminal to ask after the old leader died gets it
                    let holder_alive = self.lock_holder.is_some_and(|h| self.members.get(&h).is_some_and(|m| m.alive));
                    if holder_alive {
                        let h = self.lock_holder.unwrap();
                        self.log.push(format!("{who} connects to leader {h}"));
                        self.members.get_mut(&who).unwrap().connected_to = Some(h);
                        let outs = self.members.get_mut(&who).unwrap().node.input(Input::Connected);
                        work.extend(outs.into_iter().map(|o| (who, o)));
                    } else {
                        self.lock_holder = Some(who);
                        self.members.get_mut(&who).unwrap().connected_to = None;
                        self.log.push(format!("{who} became leader"));
                        let outs = self.members.get_mut(&who).unwrap().node.promote();
                        work.extend(outs.into_iter().map(|o| (who, o)));
                    }
                }
            }
        }
    }

    fn input(&mut self, id: ClientId, i: Input) {
        let outs = self.members.get_mut(&id).unwrap().node.input(i);
        self.process(id, outs);
    }

    // ------------------------------------------------------------------ user actions

    /// The user of terminal `id` edits its text with `ops` (already valid for what it shows).
    pub fn edit(&mut self, id: ClientId, ops: Vec<Op>) {
        let m = self.members.get_mut(&id).unwrap();
        assert!(m.alive, "editing in a dead terminal");
        ot::apply_all(&mut m.text, &ops);
        self.input(id, Input::Local(ops));
    }

    /// Insert `s` at char position `pos` (clamped) in terminal `id`.
    pub fn insert(&mut self, id: ClientId, pos: usize, s: &str) {
        let len = self.members[&id].text.chars().count();
        self.edit(id, vec![ot::ins(pos.min(len), s)]);
    }

    /// Delete up to `n` chars at `pos` (clamped) in terminal `id`.
    pub fn delete(&mut self, id: ClientId, pos: usize, n: usize) {
        let t = self.members[&id].text.clone();
        let len = t.chars().count();
        let pos = pos.min(len);
        let n = n.min(len - pos);
        if n == 0 {
            return;
        }
        let removed: String = t.chars().skip(pos).take(n).collect();
        self.edit(id, vec![ot::del(pos, &removed)]);
    }

    pub fn mark(&mut self, id: ClientId) {
        self.edit(id, vec![Op::Mark]);
    }

    pub fn discard(&mut self, id: ClientId) {
        self.input(id, Input::LocalDiscard);
    }

    pub fn undo(&mut self, id: ClientId) {
        self.input(id, Input::LocalUndo);
    }

    pub fn redo(&mut self, id: ClientId) {
        self.input(id, Input::LocalRedo);
    }

    pub fn save(&mut self, id: ClientId, force: bool) {
        self.input(id, Input::LocalSave { force });
    }

    // ------------------------------------------------------------------ scheduling

    fn nonempty(&self) -> Vec<(ClientId, ClientId)> {
        self.queues.iter().filter(|(_, q)| !q.is_empty()).map(|(k, _)| *k).collect()
    }

    pub fn pending(&self) -> usize {
        self.queues.values().map(|q| q.len()).sum()
    }

    /// Deliver one message from a randomly chosen queue (per-pair order is always preserved). False when idle.
    pub fn step(&mut self) -> bool {
        let ready = self.nonempty();
        if ready.is_empty() {
            return false;
        }
        let k = ready[self.rand_below(ready.len())];
        let w = self.queues.get_mut(&k).unwrap().pop_front().unwrap();
        let (from, to) = k;
        {
            let d = format!("{w:?}");
            self.log.push(format!("{from}->{to}: {}", d.chars().take(230).collect::<String>()));
        }
        if !self.members.get(&to).is_some_and(|m| m.alive) {
            return true; // the receiver died: the message is lost with it
        }
        let input = match w {
            Wire::ToLeader(msg) => Input::FromClient { from, msg },
            Wire::ToClient(msg) => {
                if self.members[&to].connected_to != Some(from) {
                    return true; // stale connection
                }
                Input::FromLeader(msg)
            }
            Wire::Eof => {
                self.log.push(format!(
                    "{to} sees {from} close (connected_to={:?}, leader={})",
                    self.members[&to].connected_to,
                    self.members[&to].node.is_leader()
                ));
                if self.members[&to].connected_to == Some(from) {
                    Input::LeaderLost
                } else if self.members[&to].node.is_leader() {
                    Input::ClientGone(from)
                } else {
                    return true;
                }
            }
        };
        self.input(to, input);
        true
    }

    /// Deliver everything; if a new leader is still waiting for terminals that will never come, let its window pass.
    pub fn run_until_idle(&mut self) {
        let mut guard = 0;
        loop {
            while self.step() {
                guard += 1;
                assert!(guard < 1_000_000, "the cluster never settles");
            }
            let recovering = self.leader().and_then(|n| n.leader.as_ref()).is_some_and(|l| l.is_recovering());
            if !recovering {
                break;
            }
            self.tick(super::leader::RECOVERY_WINDOW_MS + 1);
            guard += 1;
            assert!(guard < 1_000_000, "the cluster never settles");
        }
    }

    /// Advance time and run the leader's periodic work (journal flush, disk check).
    pub fn tick(&mut self, ms: u64) {
        self.time_ms += ms;
        let now = self.time_ms;
        let ids: Vec<ClientId> = self.members.iter().filter(|(_, m)| m.alive).map(|(i, _)| *i).collect();
        for id in ids {
            self.input(id, Input::Tick(now));
        }
    }

    pub fn kill(&mut self, id: ClientId) {
        let m = self.members.get_mut(&id).expect("member");
        m.alive = false;
        if self.lock_holder == Some(id) {
            self.lock_holder = None; // the kernel releases the lock when the process dies
        }
        self.log.push(format!("{id} killed"));
        // everyone connected to it sees the connection close after the data it had already sent
        let others: Vec<ClientId> = self.members.keys().copied().filter(|c| *c != id).collect();
        for c in others {
            self.push(id, c, Wire::Eof);
        }
        // data still on its way *to* the dead terminal is gone
        self.queues.retain(|(_, to), _| *to != id);
    }

    // ------------------------------------------------------------------ inspection

    pub fn alive(&self) -> Vec<ClientId> {
        self.members.iter().filter(|(_, m)| m.alive).map(|(i, _)| *i).collect()
    }

    pub fn leader(&self) -> Option<&Node> {
        self.lock_holder.and_then(|h| self.members.get(&h)).filter(|m| m.alive).map(|m| &m.node)
    }

    pub fn leader_text(&self) -> Option<String> {
        self.leader().and_then(|n| n.leader.as_ref()).map(|l| l.text())
    }

    pub fn text_of(&self, id: ClientId) -> &str {
        &self.members[&id].text
    }

    /// Everything the user could be looking at right now, per live terminal.
    pub fn texts(&self) -> Vec<(ClientId, String)> {
        self.alive().into_iter().map(|i| (i, self.members[&i].text.clone())).collect()
    }

    /// After everything has settled: each terminal shows what its replica says, all terminals agree, and they
    /// agree with the leader. Returns the common text.
    pub fn assert_converged(&self) -> String {
        let texts = self.texts();
        assert!(!texts.is_empty(), "no live terminals");
        for (id, t) in &texts {
            assert_eq!(
                t,
                &self.members[id].node.replica.text(),
                "terminal {id}: what is shown differs from its replica\nlog: {:#?}",
                self.log
            );
        }
        let first = &texts[0].1;
        for (id, t) in &texts {
            assert_eq!(t, first, "terminal {id} differs from terminal {}\nlog: {:#?}", texts[0].0, self.log);
        }
        if let Some(lt) = self.leader_text() {
            assert_eq!(&lt, first, "the leader's text differs from the terminals'\nlog: {:#?}", self.log);
        }
        for id in self.alive() {
            assert!(!self.members[&id].node.replica.has_unconfirmed(), "terminal {id} still has unconfirmed edits after settling");
        }
        first.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Cluster {
        let mut c = Cluster::new("hello world", 1);
        c.add_follower(2);
        c.run_until_idle();
        c
    }

    #[test]
    fn a_follower_joins_and_sees_the_text() {
        let c = two();
        assert_eq!(c.text_of(1), "hello world");
        assert_eq!(c.text_of(2), "hello world");
        assert_eq!(c.assert_converged(), "hello world");
    }

    #[test]
    fn edits_flow_both_ways() {
        let mut c = two();
        c.insert(1, 0, ">> ");
        c.insert(2, 14, "!");
        c.run_until_idle();
        assert_eq!(c.assert_converged(), ">> hello world!");
    }

    #[test]
    fn concurrent_typing_at_the_same_spot_converges() {
        for seed in 0..50 {
            let mut c = Cluster::new("ab", seed);
            c.add_follower(2);
            c.add_follower(3);
            c.run_until_idle();
            c.insert(1, 1, "X");
            c.insert(2, 1, "Y");
            c.insert(3, 1, "Z");
            c.run_until_idle();
            let t = c.assert_converged();
            assert!(t.len() == 5 && t.starts_with('a') && t.ends_with('b'), "{t}");
            for ch in ['X', 'Y', 'Z'] {
                assert_eq!(t.matches(ch).count(), 1);
            }
        }
    }

    #[test]
    fn edit_in_tab_1_and_2_save_in_tab_3_writes_everything() {
        let mut c = Cluster::new("0123456789", 7);
        c.add_follower(2);
        c.add_follower(3);
        c.run_until_idle();
        c.insert(1, 0, "A");
        c.insert(2, 5, "B");
        c.run_until_idle();
        c.save(3, false);
        c.run_until_idle();
        assert_eq!(c.fs.read_string(PATH).unwrap(), "A01234B56789", "the file has the edits from tabs 1 and 2");
        for id in [1, 2, 3] {
            assert!(!c.members[&id].node.replica.is_modified(), "tab {id} shows the file as saved");
            assert_eq!(c.members[&id].saved_events, 1);
        }
    }

    #[test]
    fn typing_after_a_save_makes_every_tab_modified_again() {
        let mut c = two();
        c.save(1, false);
        c.run_until_idle();
        c.insert(2, 0, "x");
        c.run_until_idle();
        assert!(c.members[&1].node.replica.is_modified() && c.members[&2].node.replica.is_modified());
    }

    #[test]
    fn undo_from_another_tab_undoes_the_last_edit_whoever_made_it() {
        let mut c = two();
        c.insert(1, 11, " one");
        c.run_until_idle();
        c.mark(1);
        c.insert(2, 15, " two");
        c.run_until_idle();
        assert_eq!(c.assert_converged(), "hello world one two");
        c.undo(1);
        c.run_until_idle();
        assert_eq!(c.assert_converged(), "hello world one");
        c.undo(2);
        c.run_until_idle();
        assert_eq!(c.assert_converged(), "hello world");
        c.redo(2);
        c.run_until_idle();
        assert_eq!(c.assert_converged(), "hello world one");
    }

    #[test]
    fn killing_a_follower_changes_nothing_for_the_others() {
        let mut c = Cluster::new("abc", 3);
        c.add_follower(2);
        c.add_follower(3);
        c.run_until_idle();
        c.insert(2, 3, "X");
        c.run_until_idle();
        c.kill(3);
        c.insert(2, 4, "Y");
        c.run_until_idle();
        assert_eq!(c.assert_converged(), "abcXY");
        assert!(c.leader().unwrap().leader.as_ref().unwrap().clients.len() == 2, "the leader dropped the dead terminal");
    }

    #[test]
    fn killing_the_leader_hands_over_without_losing_text() {
        let mut c = Cluster::new("abc", 5);
        c.add_follower(2);
        c.add_follower(3);
        c.run_until_idle();
        c.insert(2, 3, "X");
        c.run_until_idle();
        c.kill(1);
        c.run_until_idle();
        assert!(c.lock_holder.is_some() && c.lock_holder != Some(1), "a new leader took over");
        assert_eq!(c.assert_converged(), "abcX");
        c.insert(3, 0, "Q");
        c.run_until_idle();
        assert_eq!(c.assert_converged(), "QabcX", "and editing continues");
        assert!(c.log.iter().any(|l| l.contains("became leader")));
    }

    #[test]
    fn unconfirmed_edits_survive_a_leader_change() {
        // the follower's edit is still on its way when the leader dies: it is sent again after the handover
        let mut c = Cluster::new("abc", 11);
        c.add_follower(2);
        c.add_follower(3);
        c.run_until_idle();
        c.insert(2, 3, "X"); // not delivered yet
        c.kill(1);
        c.insert(3, 0, "Y");
        c.run_until_idle();
        let t = c.assert_converged();
        assert!(t.contains('X') && t.contains('Y'), "{t}");
        assert_eq!(t.len(), 5);
    }

    #[test]
    fn saving_after_a_handover_works() {
        let mut c = Cluster::new("abc", 13);
        c.add_follower(2);
        c.run_until_idle();
        c.insert(2, 3, "!");
        c.run_until_idle();
        c.kill(1);
        c.run_until_idle();
        c.save(2, false);
        c.run_until_idle();
        assert_eq!(c.fs.read_string(PATH).unwrap(), "abc!");
        assert!(!c.members[&2].node.replica.is_modified());
    }

    #[test]
    fn a_save_sent_just_as_the_leader_dies_is_not_lost() {
        for seed in 0..40 {
            let mut c = Cluster::new("abc", seed);
            c.add_follower(2);
            c.add_follower(3);
            c.run_until_idle();
            c.insert(2, 3, "!");
            c.run_until_idle();
            c.save(3, false); // the request is queued towards the leader...
            for _ in 0..(seed % 3) {
                c.step();
            }
            c.kill(1); // ...which dies before (or while) answering
            c.run_until_idle();
            assert_eq!(c.fs.read_string(PATH).unwrap(), "abc!", "seed {seed}: the save was lost");
            assert!(!c.members[&3].node.replica.is_modified(), "seed {seed}");
        }
    }

    #[test]
    fn a_save_requested_during_the_outage_runs_after_the_handover() {
        let mut c = Cluster::new("abc", 17);
        c.add_follower(2);
        c.add_follower(3);
        c.run_until_idle();
        c.insert(2, 3, "!");
        c.run_until_idle();
        c.kill(1);
        c.step(); // some terminal notices, maybe not all
        c.save(3, false);
        c.run_until_idle();
        assert_eq!(c.fs.read_string(PATH).unwrap(), "abc!", "the save was not dropped");
    }
}
