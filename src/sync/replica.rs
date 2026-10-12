//! One terminal's view of the shared document: what the leader has confirmed (`shadow`), what this terminal has
//! sent but not yet had confirmed (`in_flight`), and what it typed meanwhile (`buffered`).
//!
//! The terminal's text is always `shadow + in_flight + buffered`. Remote entries are transformed over the local
//! unconfirmed edits before they are applied, so typing never has to wait for the network.
//!
//! Sans I/O: `local*`/`request_*` and `receive` return `Effect`s for the caller to carry out.

use super::ot::{self, Op};
use super::protocol::*;
use ropey::Rope;
use std::collections::VecDeque;

/// How many committed entries a replica remembers; a new leader uses them to catch up other terminals.
pub const RECENT: usize = 512;

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Replace the whole local text (on joining).
    Replace(String),
    /// Apply these operations to the local text (already adjusted for local unconfirmed edits).
    Apply {
        ops: Vec<Op>,
        kind: Kind,
        origin: ClientId,
        focus: Option<usize>,
    },
    Send(ClientMsg),
    Saved {
        rev: u64,
    },
    SaveRefused(String),
    SaveFailed(String),
    Conflict(String),
    Reloaded,
    Notice(String),
    /// The stream has a gap or an inconsistency: the terminal must re-join to be sure of the text.
    NeedResync,
}

#[derive(Debug, Clone)]
struct Pending {
    id: MsgId,
    ops: Vec<Op>,
}

#[derive(Debug)]
pub struct Replica {
    pub id: ClientId,
    pub epoch: u64,
    pub rev: u64,
    pub shadow: Rope,
    pub saved_rev: u64,
    pub disk_hash: String,
    pub conflict: bool,
    pub joined: bool,
    /// Terminal id of the current leader, and everyone the leader last told us about.
    pub leader_id: ClientId,
    pub roster: Vec<ClientId>,
    in_flight: Option<Pending>,
    buffered: Vec<Op>,
    recent: VecDeque<Committed>,
    seq: u64,
}

pub fn apply_rope(r: &mut Rope, op: &Op) {
    match op {
        Op::Ins { pos, text } => {
            let at = (*pos).min(r.len_chars());
            r.insert(at, text);
        }
        Op::Del { pos, text } => {
            let start = (*pos).min(r.len_chars());
            let end = (start + text.chars().count()).min(r.len_chars());
            r.remove(start..end);
        }
        Op::Mark => {}
    }
}

impl Replica {
    pub fn new(id: ClientId) -> Replica {
        Replica {
            id,
            epoch: 0,
            rev: 0,
            shadow: Rope::new(),
            saved_rev: 0,
            disk_hash: String::new(),
            conflict: false,
            joined: false,
            leader_id: 0,
            roster: vec![],
            in_flight: None,
            buffered: vec![],
            recent: VecDeque::new(),
            seq: 0,
        }
    }

    fn next_id(&mut self) -> MsgId {
        self.seq += 1;
        MsgId { client: self.id, seq: self.seq }
    }

    pub fn hello(&self) -> ClientMsg {
        ClientMsg::Hello { client: self.id }
    }

    /// The text this terminal should be showing.
    pub fn text(&self) -> String {
        let mut r = self.shadow.clone();
        if let Some(f) = &self.in_flight {
            f.ops.iter().for_each(|o| apply_rope(&mut r, o));
        }
        self.buffered.iter().for_each(|o| apply_rope(&mut r, o));
        r.to_string()
    }

    /// Edits this terminal has made that the leader has not confirmed yet.
    pub fn has_unconfirmed(&self) -> bool {
        self.in_flight.is_some() || !self.buffered.is_empty()
    }

    /// Unsaved changes anywhere in the shared document, or edits still on their way.
    pub fn is_modified(&self) -> bool {
        self.rev != self.saved_rev || self.has_unconfirmed()
    }

    pub fn recent(&self) -> &VecDeque<Committed> {
        &self.recent
    }

    // ------------------------------------------------------------------ local actions

    /// The terminal changed its text. Returns what to send, if anything.
    pub fn local(&mut self, ops: Vec<Op>) -> Vec<Effect> {
        let ops = ot::compact(ops);
        if ops.iter().all(|o| matches!(o, Op::Mark)) && self.in_flight.is_none() && self.buffered.is_empty() {
            return vec![]; // a lone group boundary with nothing before it
        }
        if ops.is_empty() {
            return vec![];
        }
        match &self.in_flight {
            None => {
                let id = self.next_id();
                self.in_flight = Some(Pending { id, ops: ops.clone() });
                vec![Effect::Send(ClientMsg::Edit { id, base_rev: self.rev, ops })]
            }
            Some(_) => {
                self.buffered.extend(ops);
                self.buffered = ot::compact(std::mem::take(&mut self.buffered));
                vec![]
            }
        }
    }

    pub fn request_undo(&mut self) -> Effect {
        let id = self.next_id();
        Effect::Send(ClientMsg::Undo { id })
    }

    pub fn request_redo(&mut self) -> Effect {
        let id = self.next_id();
        Effect::Send(ClientMsg::Redo { id })
    }

    pub fn request_discard(&mut self) -> Effect {
        let id = self.next_id();
        Effect::Send(ClientMsg::Discard { id })
    }

    pub fn request_save(&mut self, force: bool) -> Effect {
        let id = self.next_id();
        Effect::Send(ClientMsg::Save { id, force })
    }

    // ------------------------------------------------------------------ messages from the leader

    pub fn receive(&mut self, msg: ServerMsg) -> Vec<Effect> {
        match msg {
            ServerMsg::Welcome { epoch, leader, rev, text, saved_rev, disk_hash, conflict, clients } => {
                self.epoch = epoch;
                self.leader_id = leader;
                self.roster = clients;
                self.rev = rev;
                self.shadow = Rope::from_str(&text);
                self.saved_rev = saved_rev;
                self.disk_hash = disk_hash;
                self.conflict = conflict;
                self.joined = true;
                self.in_flight = None;
                self.buffered.clear();
                self.recent.clear();
                vec![Effect::Replace(text)]
            }
            ServerMsg::Rejoined { epoch, leader, rev: _, saved_rev, conflict } => {
                self.epoch = epoch;
                self.leader_id = leader;
                self.joined = true;
                self.saved_rev = self.saved_rev.max(saved_rev);
                self.conflict = conflict;
                vec![]
            }
            ServerMsg::Ack { id, rev } => self.on_ack(id, rev),
            ServerMsg::Apply(c) => self.on_apply(c),
            ServerMsg::Saved { rev, disk_hash } => {
                self.saved_rev = rev;
                self.disk_hash = disk_hash;
                self.conflict = false;
                vec![Effect::Saved { rev }]
            }
            ServerMsg::SaveRefused { reason, .. } => vec![Effect::SaveRefused(reason)],
            ServerMsg::SaveFailed { reason, .. } => vec![Effect::SaveFailed(reason)],
            ServerMsg::Conflict { reason } => {
                self.conflict = true;
                vec![Effect::Conflict(reason)]
            }
            ServerMsg::Reloaded { rev, disk_hash } => {
                self.saved_rev = rev;
                self.disk_hash = disk_hash;
                self.conflict = false;
                vec![Effect::Reloaded]
            }
            ServerMsg::Notice { text } => vec![Effect::Notice(text)],
            ServerMsg::Roster { clients } => {
                self.roster = clients;
                vec![]
            }
        }
    }

    fn remember(&mut self, c: Committed) {
        self.recent.push_back(c);
        while self.recent.len() > RECENT {
            self.recent.pop_front();
        }
    }

    fn on_ack(&mut self, id: MsgId, rev: u64) -> Vec<Effect> {
        match &self.in_flight {
            Some(f) if f.id == id && rev == self.rev + 1 => {}
            // an ack we already processed (re-delivery after a leader change)
            _ if rev <= self.rev => return vec![],
            _ => return vec![Effect::NeedResync],
        }
        let f = self.in_flight.take().unwrap();
        self.commit_own(f, rev, Kind::Edit, None)
    }

    /// Our in-flight edit is now part of the confirmed history at `rev`.
    fn commit_own(&mut self, f: Pending, rev: u64, kind: Kind, focus: Option<usize>) -> Vec<Effect> {
        f.ops.iter().for_each(|o| apply_rope(&mut self.shadow, o));
        self.rev = rev;
        self.remember(Committed { rev, origin: self.id, id: Some(f.id), kind, ops: f.ops, focus });
        // send what was typed meanwhile
        if self.buffered.is_empty() {
            return vec![];
        }
        let ops = ot::compact(std::mem::take(&mut self.buffered));
        let id = self.next_id();
        self.in_flight = Some(Pending { id, ops: ops.clone() });
        vec![Effect::Send(ClientMsg::Edit { id, base_rev: self.rev, ops })]
    }

    fn on_apply(&mut self, c: Committed) -> Vec<Effect> {
        if c.rev <= self.rev {
            return vec![]; // already have it
        }
        if c.rev != self.rev + 1 {
            return vec![Effect::NeedResync];
        }
        // our own edit arriving as an entry instead of an Ack (the leader changed in between)
        if c.kind == Kind::Edit
            && c.origin == self.id
            && let (Some(id), Some(f)) = (c.id, &self.in_flight)
            && f.id == id
        {
            let f = self.in_flight.take().unwrap();
            if f.ops != c.ops {
                return vec![Effect::NeedResync];
            }
            return self.commit_own(f, c.rev, Kind::Edit, None);
        }
        // someone else's change (or an undo/redo/reload): confirm it, then adjust it for our unconfirmed edits
        c.ops.iter().for_each(|o| apply_rope(&mut self.shadow, o));
        let mut remote = c.ops.clone();
        let mut focus = c.focus;
        if let Some(f) = &mut self.in_flight {
            let (r2, f2) = ot::transform_seq(&remote, &f.ops, true);
            remote = r2;
            f.ops = f2;
            focus = focus.map(|p| through(p, &f.ops));
        }
        if !self.buffered.is_empty() {
            let (r2, b2) = ot::transform_seq(&remote, &self.buffered, true);
            remote = r2;
            self.buffered = b2;
            focus = focus.map(|p| through(p, &self.buffered));
        }
        self.rev = c.rev;
        let (kind, origin) = (c.kind, c.origin);
        self.remember(c);
        vec![Effect::Apply { ops: remote, kind, origin, focus }]
    }

    // ------------------------------------------------------------------ leader change

    /// Everything a new leader needs to carry on from this terminal's state.
    pub fn rejoin_msg(&self) -> ClientMsg {
        ClientMsg::Rejoin {
            client: self.id,
            epoch: self.epoch,
            rev: self.rev,
            recent: self.recent.iter().cloned().collect(),
            in_flight: self.in_flight.as_ref().map(|f| InFlight { id: f.id, base_rev: self.rev, ops: f.ops.clone() }),
            saved_rev: self.saved_rev,
            disk_hash: self.disk_hash.clone(),
        }
    }

    /// Forget the leader connection state until `Rejoined` arrives (unconfirmed edits are kept).
    pub fn leader_lost(&mut self) {
        self.joined = false;
    }
}

fn through(mut p: usize, ops: &[Op]) -> usize {
    for o in ops {
        p = ot::transform_pos(p, o, false);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::ot::{del, ins};
    use super::*;

    fn welcomed(id: u64, text: &str) -> Replica {
        let mut r = Replica::new(id);
        r.receive(ServerMsg::Welcome {
            epoch: 1,
            leader: 9,
            rev: 10,
            text: text.into(),
            saved_rev: 10,
            disk_hash: "h".into(),
            conflict: false,
            clients: vec![id],
        });
        r
    }

    fn sends(e: &[Effect]) -> Vec<ClientMsg> {
        e.iter().filter_map(|x| if let Effect::Send(m) = x { Some(m.clone()) } else { None }).collect()
    }

    #[test]
    fn welcome_replaces_the_text_and_marks_it_saved() {
        let r = welcomed(1, "hello");
        assert_eq!((r.rev, r.text().as_str(), r.is_modified()), (10, "hello", false));
    }

    #[test]
    fn a_local_edit_is_sent_at_once_and_later_ones_wait_for_the_confirmation() {
        let mut r = welcomed(1, "abc");
        let e = r.local(vec![ins(3, "d")]);
        let first = sends(&e);
        assert!(matches!(&first[..], [ClientMsg::Edit { base_rev: 10, .. }]));
        assert!(r.local(vec![ins(4, "e")]).is_empty(), "a second edit is held back while one is in flight");
        assert!(r.local(vec![ins(5, "f")]).is_empty());
        assert_eq!(r.text(), "abcdef");
        assert!(r.is_modified());
        let ClientMsg::Edit { id, .. } = first[0] else { panic!() };
        let e = r.receive(ServerMsg::Ack { id, rev: 11 });
        let next = sends(&e);
        let [ClientMsg::Edit { base_rev, ops, .. }] = &next[..] else { panic!("{next:?}") };
        assert_eq!(*base_rev, 11, "the held-back edits go out against the new revision");
        assert_eq!(ops, &vec![ins(4, "ef")], "and as one merged edit");
        assert_eq!(r.shadow.to_string(), "abcd");
    }

    #[test]
    fn a_remote_edit_is_adjusted_for_unconfirmed_local_edits() {
        let mut r = welcomed(1, "hello world");
        r.local(vec![ins(0, ">> ")]); // in flight
        r.local(vec![ins(14, "!")]); // buffered
        let e = r.receive(ServerMsg::Apply(Committed {
            rev: 11,
            origin: 2,
            id: None,
            kind: Kind::Edit,
            ops: vec![del(6, "world"), ins(6, "there")],
            focus: None,
        }));
        let [Effect::Apply { ops, .. }] = &e[..] else { panic!("{e:?}") };
        let mut shown = String::from(">> hello world!");
        ot::apply_all(&mut shown, ops);
        assert_eq!(shown, ">> hello there!", "the remote change lands in the right place");
        assert_eq!(r.text(), ">> hello there!");
        assert_eq!(r.rev, 11);
        assert_eq!(r.shadow.to_string(), "hello there");
    }

    #[test]
    fn duplicates_are_ignored_and_gaps_ask_for_a_resync() {
        let mut r = welcomed(1, "abc");
        let c = |rev| Committed { rev, origin: 2, id: None, kind: Kind::Edit, ops: vec![ins(0, "x")], focus: None };
        assert_eq!(r.receive(ServerMsg::Apply(c(11))).len(), 1);
        assert!(r.receive(ServerMsg::Apply(c(11))).is_empty(), "already applied");
        assert!(r.receive(ServerMsg::Apply(c(5))).is_empty(), "old");
        assert_eq!(r.receive(ServerMsg::Apply(c(13))), vec![Effect::NeedResync], "revision 12 is missing");
        assert_eq!(r.text(), "xabc");
    }

    #[test]
    fn an_ack_for_something_we_never_sent_is_a_resync_not_corruption() {
        let mut r = welcomed(1, "abc");
        let e = r.receive(ServerMsg::Ack { id: MsgId { client: 1, seq: 99 }, rev: 11 });
        assert_eq!(e, vec![Effect::NeedResync]);
        assert_eq!(r.text(), "abc");
    }

    #[test]
    fn our_own_edit_arriving_as_an_entry_counts_as_its_confirmation() {
        let mut r = welcomed(1, "abc");
        let e = r.local(vec![ins(3, "d")]);
        let ClientMsg::Edit { id, ops, .. } = sends(&e).remove(0) else { panic!() };
        // after a leader change the confirmation can arrive as a plain entry
        let e = r.receive(ServerMsg::Apply(Committed { rev: 11, origin: 1, id: Some(id), kind: Kind::Edit, ops, focus: None }));
        assert!(e.is_empty(), "nothing to apply: the text already shows it");
        assert_eq!((r.rev, r.shadow.to_string(), r.has_unconfirmed()), (11, "abcd".to_string(), false));
    }

    #[test]
    fn rejoin_carries_the_unconfirmed_edit_against_the_current_revision() {
        let mut r = welcomed(1, "abc");
        r.local(vec![ins(0, "X")]);
        r.receive(ServerMsg::Apply(Committed { rev: 11, origin: 2, id: None, kind: Kind::Edit, ops: vec![ins(0, "Q")], focus: None }));
        let ClientMsg::Rejoin { rev, in_flight, recent, .. } = r.rejoin_msg() else { panic!() };
        assert_eq!(rev, 11);
        let f = in_flight.unwrap();
        assert_eq!(f.base_rev, 11, "expressed against what we have confirmed");
        assert_eq!(f.ops, vec![ins(1, "X")], "and already shifted past the remote insert");
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn undo_focus_follows_unconfirmed_edits() {
        let mut r = welcomed(1, "abcdef");
        r.local(vec![ins(0, "XX")]);
        let e = r.receive(ServerMsg::Apply(Committed {
            rev: 11,
            origin: 1,
            id: Some(MsgId { client: 1, seq: 50 }),
            kind: Kind::Undo,
            ops: vec![del(3, "d")],
            focus: Some(3),
        }));
        let [Effect::Apply { focus, .. }] = &e[..] else { panic!("{e:?}") };
        assert_eq!(*focus, Some(5), "the cursor target moves past our two unconfirmed characters");
    }

    #[test]
    fn saved_and_conflict_flags() {
        let mut r = welcomed(1, "abc");
        r.receive(ServerMsg::Conflict { reason: "disk".into() });
        assert!(r.conflict);
        let e = r.receive(ServerMsg::Saved { rev: 10, disk_hash: "new".into() });
        assert_eq!(e, vec![Effect::Saved { rev: 10 }]);
        assert!(!r.conflict && !r.is_modified());
    }
}
