//! One terminal's participation in live sync: it is either the leader (and also a client of itself) or a
//! follower, and it survives the leader changing. Sans I/O: feed it [`Input`]s, carry out its [`Output`]s.
//!
//! Leader change in short: a follower whose connection to the leader closes keeps its text and unconfirmed
//! edits, asks the runtime to try to take over (`Output::TryElection`, backed by an OS file lock that the kernel
//! releases when the old leader dies). The winner calls [`Node::promote`]; the others connect to it and send a
//! `Rejoin` carrying what they know, which the new leader merges.

use super::leader::{Dest, LeaderCore, Out};
use super::ot::Op;
use super::protocol::*;
use super::replica::{Effect, Replica};
use super::storage::{Fs, Journal};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Connected to a leader (or being one).
    Live,
    /// The leader is gone; waiting for a new one. Edits are kept and sent after the rejoin.
    Electing,
    /// No leader could be found or started: the text is local only.
    Standalone,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Local(Vec<Op>),
    LocalUndo,
    LocalRedo,
    LocalSave {
        force: bool,
    },
    FromLeader(ServerMsg),
    FromClient {
        from: ClientId,
        msg: ClientMsg,
    },
    /// The leader connection closed (follower) .
    LeaderLost,
    /// A follower's connection closed (leader).
    ClientGone(ClientId),
    /// The runtime connected to a (new) leader: introduce ourselves.
    Connected,
    /// The runtime could not find or become a leader.
    ElectionFailed,
    Tick(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    ToLeader(ClientMsg),
    ToClient(ClientId, ServerMsg),
    /// Something for the editor to show or apply.
    Editor(Effect),
    /// Ask the runtime to try to become leader (or find the new one) and report back.
    TryElection,
    /// Leader-side notices (recovered edits and similar).
    Notice(String),
}

pub type FsFactory = Box<dyn Fn() -> Box<dyn Fs> + Send>;
pub type JournalFactory = Box<dyn Fn() -> Box<dyn Journal> + Send>;

pub struct Node {
    pub id: ClientId,
    pub replica: Replica,
    pub leader: Option<LeaderCore>,
    pub state: State,
    path: PathBuf,
    fs_factory: FsFactory,
    journal_factory: JournalFactory,
    now_ms: u64,
    /// A save requested while there was no leader: run it once a leader exists.
    pending_save: Option<bool>,
}

impl Node {
    /// Start as the leader of `path`.
    pub fn new_leader(
        id: ClientId,
        path: PathBuf,
        fs_factory: FsFactory,
        journal_factory: JournalFactory,
        now_ms: u64,
    ) -> std::io::Result<(Node, Vec<Output>)> {
        let mut core = LeaderCore::open(id, path.clone(), fs_factory(), journal_factory(), 1, now_ms)?;
        let notices = core.take_notices();
        let mut node = Node {
            id,
            replica: Replica::new(id),
            leader: Some(core),
            state: State::Live,
            path,
            fs_factory,
            journal_factory,
            now_ms,
            pending_save: None,
        };
        let mut out: Vec<Output> = notices.into_iter().map(Output::Notice).collect();
        let outs = node.leader.as_mut().unwrap().join(id);
        out.extend(node.route(outs));
        Ok((node, out))
    }

    pub fn is_leader(&self) -> bool {
        self.leader.is_some()
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Deliver `outs` from the leader core: ones for ourselves go to our replica, the rest to the runtime.
    fn route(&mut self, outs: Vec<Out>) -> Vec<Output> {
        let mut result = vec![];
        let mut queue: std::collections::VecDeque<Out> = outs.into();
        while let Some(o) = queue.pop_front() {
            let mut mine = vec![];
            let clients: Vec<ClientId> = self.leader.as_ref().map(|l| l.clients.iter().copied().collect()).unwrap_or_default();
            let targets: Vec<ClientId> = match &o.to {
                Dest::One(c) => vec![*c],
                Dest::All => clients.clone(),
                Dest::AllExcept(x) => clients.iter().copied().filter(|c| c != x).collect(),
            };
            for t in targets {
                if t == self.id {
                    mine.push(o.msg.clone());
                } else {
                    result.push(Output::ToClient(t, o.msg.clone()));
                }
            }
            // a message to ourselves may produce effects that ask the leader for something (sending buffered edits)
            for m in mine {
                let effects = self.replica.receive(m);
                for e in effects {
                    match e {
                        Effect::Send(msg) => queue.extend(self.leader.as_mut().unwrap().handle(self.id, msg)),
                        other => result.push(Output::Editor(other)),
                    }
                }
            }
        }
        result
    }

    /// Apply replica effects: sends go to the leader (or are handled locally when we are the leader).
    fn effects(&mut self, effects: Vec<Effect>) -> Vec<Output> {
        let mut out = vec![];
        for e in effects {
            match e {
                Effect::Send(msg) => match (&mut self.leader, self.state) {
                    (Some(l), _) => {
                        let outs = l.handle(self.id, msg);
                        out.extend(self.route(outs));
                    }
                    (None, State::Live) => out.push(Output::ToLeader(msg)),
                    // while electing, the rejoin carries the unconfirmed edit; in standalone mode it stays local
                    (None, _) => {}
                },
                other => out.push(Output::Editor(other)),
            }
        }
        out
    }

    pub fn input(&mut self, input: Input) -> Vec<Output> {
        match input {
            Input::Local(ops) => {
                let eff = self.replica.local(ops);
                self.effects(eff)
            }
            Input::LocalUndo | Input::LocalRedo => {
                if self.state != State::Live {
                    return vec![Output::Editor(Effect::Notice("sync is re-connecting: try again in a moment".into()))];
                }
                let e = if input == Input::LocalUndo { self.replica.request_undo() } else { self.replica.request_redo() };
                self.effects(vec![e])
            }
            Input::LocalSave { force } => match self.state {
                State::Live => {
                    let e = self.replica.request_save(force);
                    self.effects(vec![e])
                }
                State::Electing => {
                    self.pending_save = Some(force);
                    vec![Output::Editor(Effect::Notice("saving as soon as sync reconnects…".into()))]
                }
                State::Standalone => vec![Output::Editor(Effect::SaveFailed("sync is not connected: saving locally".into()))],
            },
            Input::FromLeader(msg) => {
                let eff = self.replica.receive(msg);
                let mut out = self.effects(eff);
                // a save asked for during the outage goes out once we are connected again
                if self.state == State::Live
                    && self.replica.joined
                    && let Some(force) = self.pending_save.take()
                {
                    let e = self.replica.request_save(force);
                    out.extend(self.effects(vec![e]));
                }
                out
            }
            Input::FromClient { from, msg } => match self.leader.as_mut() {
                Some(l) => {
                    let outs = l.handle(from, msg);
                    self.route(outs)
                }
                None => vec![],
            },
            Input::ClientGone(c) => match self.leader.as_mut() {
                Some(l) => {
                    let outs = l.leave(c);
                    self.route(outs)
                }
                None => vec![],
            },
            Input::LeaderLost => {
                if self.leader.is_some() {
                    return vec![];
                }
                self.state = State::Electing;
                self.replica.leader_lost();
                vec![Output::Editor(Effect::Notice("sync leader lost: re-connecting…".into())), Output::TryElection]
            }
            Input::Connected => {
                self.state = State::Live;
                let msg = if self.replica.rev == 0 && !self.replica.joined && self.replica.epoch == 0 {
                    self.replica.hello()
                } else {
                    self.replica.rejoin_msg()
                };
                vec![Output::ToLeader(msg)]
            }
            Input::ElectionFailed => {
                self.state = State::Standalone;
                vec![Output::Editor(Effect::Notice("live sync lost: editing locally; save with care".into()))]
            }
            Input::Tick(now) => {
                self.now_ms = now;
                match self.leader.as_mut() {
                    Some(l) => {
                        let outs = l.tick(now);
                        let mut out = self.route(outs);
                        let notices = self.leader.as_mut().unwrap().take_notices();
                        out.extend(notices.into_iter().map(Output::Notice));
                        out
                    }
                    None => vec![],
                }
            }
        }
    }

    /// The runtime won the leader lock: become the leader from our own state.
    pub fn promote(&mut self) -> Vec<Output> {
        let r = &self.replica;
        // the terminals that should rejoin: everyone the old leader told us about except it and ourselves
        let expected: std::collections::BTreeSet<ClientId> =
            r.roster.iter().copied().filter(|c| *c != r.leader_id && *c != self.id).collect();
        let mut core = LeaderCore::resume(
            self.id,
            expected,
            self.path.clone(),
            (self.fs_factory)(),
            (self.journal_factory)(),
            r.epoch + 1,
            r.rev,
            r.shadow.clone(),
            r.recent().iter().cloned().collect(),
            r.saved_rev,
            r.disk_hash.clone(),
            r.conflict,
            self.now_ms,
        );
        let notices = core.take_notices();
        self.leader = Some(core);
        self.state = State::Live;
        let mut out: Vec<Output> = notices.into_iter().map(Output::Notice).collect();
        // we are the first terminal to rejoin
        let rejoin = self.replica.rejoin_msg();
        let outs = self.leader.as_mut().unwrap().handle(self.id, rejoin);
        out.extend(self.route(outs));
        if let Some(force) = self.pending_save.take() {
            let e = self.replica.request_save(force);
            out.extend(self.effects(vec![e]));
        }
        out
    }
}

impl Node {
    /// Start as a follower; the runtime then connects and calls `input(Connected)`.
    pub fn follower(id: ClientId, path: PathBuf, fs_factory: FsFactory, journal_factory: JournalFactory) -> Node {
        Node {
            id,
            replica: Replica::new(id),
            leader: None,
            state: State::Live,
            path,
            fs_factory,
            journal_factory,
            now_ms: 0,
            pending_save: None,
        }
    }
}
