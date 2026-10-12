//! The leader: puts every edit in one order, keeps the shared undo history, saves the file, and notices when the
//! file changes underneath. Sans I/O: `handle` and `tick` return messages for the caller to deliver.
//!
//! One leader exists per file at a time (the caller guarantees that with an OS file lock). If it dies, another
//! terminal builds a new `LeaderCore` from its own replica with [`LeaderCore::resume`] and carries on.

use super::ot::{self, Op};
use super::protocol::*;
use super::replica::apply_rope;
use super::storage::*;
use ropey::Rope;
use std::collections::{BTreeSet, VecDeque};
use std::io;
use std::path::PathBuf;

/// Author of changes that come from outside any terminal (reloading the file).
pub const SYSTEM: ClientId = 0;
/// History kept for transforming late edits and catching up reconnecting terminals.
pub const LOG_CAP: usize = 4096;
const JOURNAL_FLUSH_MS: u64 = 1000;
const DISK_CHECK_MS: u64 = 500;
/// The journal is only compacted once it is at least this big (and 4x the document).
pub const JOURNAL_COMPACT_MIN: usize = 1 << 20;

type Group = Vec<Op>;

#[derive(Debug, Clone, PartialEq)]
pub enum Dest {
    One(ClientId),
    All,
    AllExcept(ClientId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Out {
    pub to: Dest,
    pub msg: ServerMsg,
}

fn one(to: ClientId, msg: ServerMsg) -> Out {
    Out { to: Dest::One(to), msg }
}

fn all(msg: ServerMsg) -> Out {
    Out { to: Dest::All, msg }
}

/// How long a new leader waits for the other terminals before carrying on.
pub const RECOVERY_WINDOW_MS: u64 = 600;

/// A new leader after a failover is collecting what every terminal knows. Until it is done it commits nothing,
/// so the history cannot fork.
struct Recovery {
    until_ms: u64,
    expected: BTreeSet<ClientId>,
    /// Terminals that have rejoined, in arrival order.
    rejoined: Vec<Rejoin>,
    staged: std::collections::BTreeMap<u64, Committed>,
    queued: Vec<(ClientId, ClientMsg)>,
}

struct Rejoin {
    client: ClientId,
    rev: u64,
    in_flight: Option<InFlight>,
}

pub struct LeaderCore {
    /// This terminal's own id (the leader is also a client).
    pub me: ClientId,
    pub epoch: u64,
    pub rev: u64,
    doc: Rope,
    log: VecDeque<Committed>,
    pub clients: BTreeSet<ClientId>,
    /// Inverse operation groups, newest last; the top one applies to the current text.
    undo: Vec<Group>,
    redo: Vec<Group>,
    /// Origin of the undo group that new edits from the same terminal extend (typing runs).
    open_group: Option<ClientId>,
    path: PathBuf,
    pub saved_rev: u64,
    pub disk_hash: String,
    disk_stamp: Option<Stamp>,
    pub conflict: bool,
    fs: Box<dyn Fs>,
    journal: Box<dyn Journal>,
    journal_dirty: bool,
    /// Roughly how much has been appended to the journal since it was last reset or compacted.
    journal_bytes: usize,
    last_flush_ms: u64,
    last_disk_check_ms: u64,
    notices: Vec<String>,
    /// How many unsaved edits were recovered from a journal when opening.
    pub recovered: Option<usize>,
    recovery: Option<Recovery>,
}

fn valid_utf8(bytes: &[u8]) -> io::Result<String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "file is not valid UTF-8"))
}

fn invert(group: &[Op]) -> Group {
    group.iter().rev().map(Op::inverse).collect()
}

fn focus_of(ops: &[Op]) -> Option<usize> {
    ops.iter().rev().find_map(|o| match o {
        Op::Ins { pos, text } => Some(pos + text.chars().count()),
        Op::Del { pos, .. } => Some(*pos),
        Op::Mark => None,
    })
}

impl LeaderCore {
    /// Start a fresh leader for `path`, recovering unsaved edits from a leftover journal if it matches the file.
    pub fn open(
        me: ClientId,
        path: PathBuf,
        fs: Box<dyn Fs>,
        mut journal: Box<dyn Journal>,
        epoch: u64,
        now_ms: u64,
    ) -> io::Result<LeaderCore> {
        let stamp = fs.stamp(&path)?;
        let bytes = if stamp.is_some() { fs.read(&path)? } else { vec![] };
        let text = valid_utf8(&bytes)?;
        let disk_hash = hash_bytes(&bytes);
        let mut core = LeaderCore {
            me,
            epoch,
            rev: 0,
            doc: Rope::from_str(&text),
            log: VecDeque::new(),
            clients: BTreeSet::new(),
            undo: vec![],
            redo: vec![],
            open_group: None,
            path,
            saved_rev: 0,
            disk_hash: disk_hash.clone(),
            disk_stamp: stamp,
            conflict: false,
            fs,
            journal: Box::new(NullJournal),
            journal_dirty: false,
            journal_bytes: 0,
            last_flush_ms: now_ms,
            last_disk_check_ms: now_ms,
            notices: vec![],
            recovered: None,
            recovery: None,
        };
        let mut base_rev = 0;
        if let Some(j) = journal.load()? {
            if j.base_hash == disk_hash {
                base_rev = j.base_rev;
                core.rev = base_rev;
                core.saved_rev = base_rev;
                let mut n = 0;
                for e in j.entries {
                    if e.rev != core.rev + 1 {
                        break;
                    }
                    e.ops.iter().for_each(|o| apply_rope(&mut core.doc, o));
                    core.rev = e.rev;
                    core.push_log(e);
                    n += 1;
                }
                if n > 0 {
                    core.recovered = Some(n);
                    core.notices.push(format!("recovered {n} unsaved edit{} from the last session", if n == 1 { "" } else { "s" }));
                }
            } else if !j.entries.is_empty() {
                core.notices.push("an earlier session left unsaved edits, but the file has changed since: they were not applied".into());
            }
        }
        if core.recovered.is_none() {
            journal.reset(&disk_hash, base_rev)?;
        }
        core.journal = journal;
        Ok(core)
    }

    /// Take over after the previous leader died, from this terminal's confirmed state.
    #[allow(clippy::too_many_arguments)]
    pub fn resume(
        me: ClientId,
        expected: BTreeSet<ClientId>,
        path: PathBuf,
        fs: Box<dyn Fs>,
        journal: Box<dyn Journal>,
        epoch: u64,
        rev: u64,
        shadow: Rope,
        recent: Vec<Committed>,
        saved_rev: u64,
        disk_hash: String,
        conflict: bool,
        now_ms: u64,
    ) -> LeaderCore {
        let mut core = LeaderCore {
            me,
            epoch,
            rev,
            doc: shadow,
            log: VecDeque::new(),
            clients: BTreeSet::new(),
            undo: vec![],
            redo: vec![],
            open_group: None,
            path,
            saved_rev,
            disk_hash,
            disk_stamp: None, // unknown: the first disk check decides what happened to the file meanwhile
            conflict,
            fs,
            journal,
            journal_dirty: false,
            journal_bytes: 0,
            last_flush_ms: now_ms,
            last_disk_check_ms: 0,
            notices: vec![],
            recovered: None,
            recovery: Some(Recovery {
                until_ms: now_ms + RECOVERY_WINDOW_MS,
                expected,
                rejoined: vec![],
                staged: Default::default(),
                queued: vec![],
            }),
        };
        recent.into_iter().filter(|e| e.rev <= rev).for_each(|e| core.push_log(e));
        // edits the dead leader journalled but never got out to anyone
        if let Ok(Some(j)) = core.journal.load() {
            let mut n = 0;
            let start = core.rev;
            for e in j.entries.into_iter().filter(|e| e.rev > start) {
                if e.rev != core.rev + 1 {
                    break;
                }
                e.ops.iter().for_each(|o| apply_rope(&mut core.doc, o));
                core.rev = e.rev;
                core.push_log(e);
                n += 1;
            }
            if n > 0 {
                core.notices.push(format!("recovered {n} edit{} from the journal", if n == 1 { "" } else { "s" }));
            }
        }
        core
    }

    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    pub fn doc(&self) -> &Rope {
        &self.doc
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// True while a new leader is still collecting what the other terminals know.
    pub fn is_recovering(&self) -> bool {
        self.recovery.is_some()
    }

    pub fn is_dirty(&self) -> bool {
        self.rev != self.saved_rev
    }

    fn push_log(&mut self, e: Committed) {
        self.log.push_back(e);
        while self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
    }

    fn welcome(&self) -> ServerMsg {
        ServerMsg::Welcome {
            epoch: self.epoch,
            leader: self.me,
            rev: self.rev,
            text: self.doc.to_string(),
            saved_rev: self.saved_rev,
            disk_hash: self.disk_hash.clone(),
            conflict: self.conflict,
            clients: self.clients.iter().copied().collect(),
        }
    }

    fn roster(&self) -> Out {
        all(ServerMsg::Roster { clients: self.clients.iter().copied().collect() })
    }

    // ------------------------------------------------------------------ membership

    pub fn join(&mut self, c: ClientId) -> Vec<Out> {
        self.clients.insert(c);
        vec![one(c, self.welcome()), self.roster()]
    }

    pub fn leave(&mut self, c: ClientId) -> Vec<Out> {
        if self.clients.remove(&c) { vec![self.roster()] } else { vec![] }
    }

    fn resync(&self, c: ClientId, why: &str) -> Vec<Out> {
        vec![one(c, ServerMsg::Notice { text: format!("re-synchronising: {why}") }), one(c, self.welcome())]
    }

    // ------------------------------------------------------------------ messages

    pub fn handle(&mut self, from: ClientId, msg: ClientMsg) -> Vec<Out> {
        if self.recovery.is_some() {
            return self.handle_while_recovering(from, msg);
        }
        match msg {
            ClientMsg::Hello { client } => self.join(client),
            ClientMsg::Bye { client } => self.leave(client),
            ClientMsg::Edit { id, base_rev, ops } => self.on_edit(from, id, base_rev, ops),
            ClientMsg::Undo { id } => self.on_undo_redo(from, id, true),
            ClientMsg::Redo { id } => self.on_undo_redo(from, id, false),
            ClientMsg::Save { id, force } => self.on_save(from, id, force),
            ClientMsg::Discard { id } => self.on_discard(from, id),
            ClientMsg::Rejoin { client, rev, recent, in_flight, saved_rev, disk_hash, .. } => {
                self.stage_rejoin(client, recent, saved_rev, disk_hash);
                self.respond_rejoin(Rejoin { client, rev, in_flight })
            }
        }
    }

    /// Check that `ops` can be applied to the current text (positions in range and deleted text matching).
    /// Returns the resulting text on success.
    fn try_apply(&self, ops: &[Op]) -> Result<Rope, String> {
        let mut r = self.doc.clone();
        for o in ops {
            match o {
                Op::Ins { pos, .. } if *pos > r.len_chars() => return Err(format!("insert at {pos} beyond the end ({})", r.len_chars())),
                Op::Del { pos, text } => {
                    let end = pos + text.chars().count();
                    if end > r.len_chars() {
                        return Err(format!("delete {pos}..{end} beyond the end ({})", r.len_chars()));
                    }
                    if r.slice(*pos..end) != text.as_str() {
                        return Err(format!("delete at {pos} does not match the text there"));
                    }
                }
                _ => {}
            }
            apply_rope(&mut r, o);
        }
        Ok(r)
    }

    fn commit(&mut self, origin: ClientId, id: Option<MsgId>, kind: Kind, ops: Vec<Op>, new_doc: Rope) -> Committed {
        self.doc = new_doc;
        self.rev += 1;
        let focus = if matches!(kind, Kind::Undo | Kind::Redo) { focus_of(&ops) } else { None };
        let entry = Committed { rev: self.rev, origin, id, kind, ops, focus };
        self.push_log(entry.clone());
        if self.journal.append(&entry).is_err() {
            self.notices.push("could not write the recovery journal".into());
        }
        self.journal_dirty = true;
        self.journal_bytes += entry
            .ops
            .iter()
            .map(|o| match o {
                Op::Ins { text, .. } | Op::Del { text, .. } => text.len() + 24,
                Op::Mark => 8,
            })
            .sum::<usize>()
            + 64;
        entry
    }

    fn on_edit(&mut self, from: ClientId, id: MsgId, base_rev: u64, ops: Vec<Op>) -> Vec<Out> {
        if let Some(e) = self.log.iter().find(|e| e.id == Some(id)) {
            // already committed (a re-send after a failover): confirm again instead of applying twice
            return vec![one(from, ServerMsg::Ack { id, rev: e.rev })];
        }
        let oldest = self.log.front().map(|e| e.rev).unwrap_or(self.rev + 1);
        if base_rev > self.rev || base_rev + 1 < oldest {
            return self.resync(from, "your edit is too old to merge");
        }
        // bring the edit up to date: it loses ties against anything committed before it
        let mut ops = ops;
        for e in self.log.iter().filter(|e| e.rev > base_rev) {
            ops = ot::transform_seq(&ops, &e.ops, false).0;
        }
        let new_doc = match self.try_apply(&ops) {
            Ok(d) => d,
            Err(why) => return self.resync(from, &why),
        };
        self.note_edit(from, &ops);
        let entry = self.commit(from, Some(id), Kind::Edit, ops, new_doc);
        vec![one(from, ServerMsg::Ack { id, rev: entry.rev }), Out { to: Dest::AllExcept(from), msg: ServerMsg::Apply(entry) }]
    }

    // ------------------------------------------------------------------ shared undo / redo

    /// Record the inverse of a committed edit on the undo stack and keep older entries applicable.
    fn note_edit(&mut self, origin: ClientId, ops: &[Op]) {
        self.redo.clear();
        let mut first = true;
        let mut cur: Vec<Op> = vec![];
        let flush = |this: &mut LeaderCore, seg: &mut Vec<Op>, extend: bool| {
            if seg.is_empty() {
                return;
            }
            this.note_segment(seg, extend);
            seg.clear();
        };
        for o in ops {
            if matches!(o, Op::Mark) {
                let extend = first && self.open_group == Some(origin);
                flush(self, &mut cur, extend);
                first = false;
                self.open_group = None;
            } else {
                cur.push(o.clone());
            }
        }
        let extend = first && self.open_group == Some(origin);
        let had = !cur.is_empty();
        flush(self, &mut cur, extend);
        if had {
            self.open_group = Some(origin);
        }
    }

    /// One mark-free run of operations (already applied to the document).
    ///
    /// The history is one line (the leader commits in a single total order) and undo is last-in-first-out, so
    /// each stored inverse is exactly valid when its turn comes: no transformation is needed.
    fn note_segment(&mut self, seg: &[Op], extend: bool) {
        let mut inverse = invert(seg);
        match self.undo.last_mut() {
            Some(top) if extend => {
                // the edit joins the open group: undo it first, then the group as it was
                inverse.append(top);
                *top = inverse;
            }
            _ => self.undo.push(inverse),
        }
    }

    fn on_undo_redo(&mut self, from: ClientId, id: MsgId, undo: bool) -> Vec<Out> {
        if let Some(e) = self.log.iter().find(|e| e.id == Some(id)) {
            return vec![one(from, ServerMsg::Apply(e.clone()))]; // re-send after failover
        }
        let popped = if undo { self.undo.pop() } else { self.redo.pop() };
        let Some(group) = popped else {
            let what = if undo { "already at the oldest change" } else { "nothing to redo" };
            return vec![one(from, ServerMsg::Notice { text: what.into() })];
        };
        let new_doc = match self.try_apply(&group) {
            Ok(d) => d,
            Err(why) => {
                self.undo.clear();
                self.redo.clear();
                return self.resync(from, &format!("undo history out of date ({why})"));
            }
        };
        // what undoes this undo goes on the other stack
        let back = invert(&group);
        if undo {
            self.redo.push(back);
        } else {
            self.undo.push(back);
        }
        self.open_group = None;
        let kind = if undo { Kind::Undo } else { Kind::Redo };
        let entry = self.commit(from, Some(id), kind, group, new_doc);
        vec![all(ServerMsg::Apply(entry))]
    }

    // ------------------------------------------------------------------ saving

    fn on_save(&mut self, from: ClientId, id: MsgId, force: bool) -> Vec<Out> {
        if !force {
            if self.conflict {
                return vec![one(
                    from,
                    ServerMsg::SaveRefused {
                        id,
                        reason: "the file changed on disk while you have unsaved edits; :w! overwrites it".into(),
                    },
                )];
            }
            if let Ok(Some(stamp)) = self.fs.stamp(&self.path)
                && self.disk_stamp.as_ref() != Some(&stamp)
            {
                let on_disk = self.fs.read(&self.path).map(|b| hash_bytes(&b)).unwrap_or_default();
                if on_disk != self.disk_hash {
                    self.conflict = true;
                    return vec![one(
                        from,
                        ServerMsg::SaveRefused { id, reason: "the file changed on disk since it was loaded; :w! overwrites it".into() },
                    )];
                }
                self.disk_stamp = Some(stamp);
            }
        }
        let bytes = self.doc.to_string().into_bytes();
        if let Err(e) = self.fs.write_atomic(&self.path, &bytes) {
            return vec![one(from, ServerMsg::SaveFailed { id, reason: e.to_string() })];
        }
        self.disk_hash = hash_bytes(&bytes);
        self.disk_stamp = self.fs.stamp(&self.path).ok().flatten();
        self.saved_rev = self.rev;
        self.conflict = false;
        if self.journal.reset(&self.disk_hash, self.rev).is_err() {
            self.notices.push("could not reset the recovery journal".into());
        }
        self.journal_dirty = false;
        self.journal_bytes = 0;
        vec![all(ServerMsg::Saved { rev: self.rev, disk_hash: self.disk_hash.clone() })]
    }

    /// Hard undo of unsaved work: every terminal gets the file on disk back (as one shared edit), the undo history and the
    /// recovery journal are dropped, and the text counts as saved. Nothing is written to the file.
    fn on_discard(&mut self, from: ClientId, id: MsgId) -> Vec<Out> {
        if let Some(e) = self.log.iter().find(|e| e.id == Some(id)) {
            return vec![one(from, ServerMsg::Apply(e.clone()))]; // re-send after failover
        }
        let bytes = match self.fs.read(&self.path) {
            Ok(b) => b,
            Err(e) => return vec![one(from, ServerMsg::Notice { text: format!("cannot discard: the file cannot be read ({e})") })],
        };
        let Ok(text) = String::from_utf8(bytes.clone()) else {
            return vec![one(from, ServerMsg::Notice { text: "cannot discard: the file on disk is not valid UTF-8".into() })];
        };
        let mut out = vec![];
        if self.doc != text.as_str() {
            let old = self.doc.to_string();
            let mut ops = vec![];
            if !old.is_empty() {
                ops.push(Op::Del { pos: 0, text: old });
            }
            if !text.is_empty() {
                ops.push(Op::Ins { pos: 0, text: text.clone() });
            }
            let new_doc = Rope::from_str(&text);
            let entry = self.commit(from, Some(id), Kind::Undo, ops, new_doc);
            out.push(all(ServerMsg::Apply(entry)));
        }
        self.undo.clear();
        self.redo.clear();
        self.open_group = None;
        self.disk_hash = hash_bytes(&bytes);
        self.disk_stamp = self.fs.stamp(&self.path).ok().flatten();
        self.saved_rev = self.rev;
        self.conflict = false;
        if self.journal.reset(&self.disk_hash, self.rev).is_err() {
            self.notices.push("could not reset the recovery journal".into());
        }
        self.journal_dirty = false;
        self.journal_bytes = 0;
        out.push(all(ServerMsg::Saved { rev: self.rev, disk_hash: self.disk_hash.clone() }));
        out
    }

    // ------------------------------------------------------------------ periodic work

    /// Call regularly (a few times a second) with a monotonic millisecond clock.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Out> {
        let mut out = vec![];
        if self.recovery.as_ref().is_some_and(|r| now_ms >= r.until_ms) {
            out.extend(self.finish_recovery());
        }
        if self.journal_dirty && now_ms.saturating_sub(self.last_flush_ms) >= JOURNAL_FLUSH_MS {
            self.flush_journal(now_ms);
        }
        self.maybe_compact_journal();
        if now_ms.saturating_sub(self.last_disk_check_ms) >= DISK_CHECK_MS {
            self.last_disk_check_ms = now_ms;
            out.extend(self.check_disk());
        }
        out
    }

    /// Make the journal durable now (before quitting, and in tests).
    pub fn flush_journal(&mut self, now_ms: u64) {
        let _ = self.journal.flush();
        self.journal_dirty = false;
        self.last_flush_ms = now_ms;
    }

    /// Keep the journal small: when it has grown well past the size of the document, replace it with the one
    /// change that turns the file on disk into the current text.
    fn maybe_compact_journal(&mut self) {
        let limit = (4 * self.doc.len_bytes()).max(JOURNAL_COMPACT_MIN);
        if self.journal_bytes <= limit || self.recovery.is_some() || self.rev == 0 {
            return;
        }
        let Ok(bytes) = self.fs.read(&self.path) else { return };
        if hash_bytes(&bytes) != self.disk_hash {
            return; // the file moved on: the journal's base is no longer what is on disk
        }
        let Ok(base_text) = valid_utf8(&bytes) else { return };
        let ops = ot::diff_ops(&base_text, &self.doc.to_string());
        let entries = if ops.is_empty() {
            vec![]
        } else {
            vec![Committed { rev: self.rev, origin: SYSTEM, id: None, kind: Kind::Edit, ops, focus: None }]
        };
        let base_rev = if entries.is_empty() { self.rev } else { self.rev - 1 };
        if self.journal.replace(&self.disk_hash, base_rev, &entries).is_ok() {
            self.journal_bytes = entries
                .iter()
                .map(|e| {
                    e.ops.iter().map(|o| if let Op::Ins { text, .. } | Op::Del { text, .. } = o { text.len() } else { 0 }).sum::<usize>()
                })
                .sum();
            self.journal_dirty = false;
        }
    }

    fn check_disk(&mut self) -> Vec<Out> {
        let Ok(Some(stamp)) = self.fs.stamp(&self.path) else { return vec![] };
        if self.disk_stamp.as_ref() == Some(&stamp) {
            return vec![];
        }
        self.disk_stamp = Some(stamp);
        let Ok(bytes) = self.fs.read(&self.path) else { return vec![] };
        let h = hash_bytes(&bytes);
        if h == self.disk_hash {
            return vec![]; // touched, not changed
        }
        // the old leader may have saved just before dying: the file then equals our current text
        if h == hash_rope(&self.doc) {
            self.disk_hash = h.clone();
            self.saved_rev = self.rev;
            self.conflict = false;
            let _ = self.journal.reset(&h, self.rev);
            return vec![all(ServerMsg::Saved { rev: self.rev, disk_hash: h })];
        }
        if self.is_dirty() {
            self.conflict = true;
            return vec![all(ServerMsg::Conflict {
                reason: "the file changed on disk while you have unsaved edits; :w! overwrites it".into(),
            })];
        }
        // clean: follow the file
        let Ok(new_text) = valid_utf8(&bytes) else {
            return vec![all(ServerMsg::Notice { text: "the file on disk is no longer valid UTF-8; not reloading".into() })];
        };
        let ops = ot::diff_ops(&self.doc.to_string(), &new_text);
        if ops.is_empty() {
            return vec![];
        }
        let new_doc = Rope::from_str(&new_text);
        self.undo.clear();
        self.redo.clear();
        self.open_group = None;
        let entry = self.commit(SYSTEM, None, Kind::External, ops, new_doc);
        self.disk_hash = h.clone();
        self.saved_rev = self.rev;
        let _ = self.journal.reset(&h, self.rev);
        self.journal_dirty = false;
        vec![all(ServerMsg::Apply(entry)), all(ServerMsg::Reloaded { rev: self.rev, disk_hash: h })]
    }

    // ------------------------------------------------------------------ failover

    /// Take in what a rejoining terminal knows: committed entries we lack (adopted once they are contiguous) and
    /// which revision of the file it believes is saved.
    fn stage_rejoin(&mut self, client: ClientId, recent: Vec<Committed>, saved_rev: u64, disk_hash: String) {
        self.clients.insert(client);
        let mut staged = self.recovery.as_mut().map(|r| std::mem::take(&mut r.staged)).unwrap_or_default();
        for e in recent.into_iter().filter(|e| e.rev > self.rev) {
            staged.entry(e.rev).or_insert(e);
        }
        let mut adopted = 0;
        while let Some(e) = staged.remove(&(self.rev + 1)) {
            e.ops.iter().for_each(|o| apply_rope(&mut self.doc, o));
            self.rev = e.rev;
            let _ = self.journal.append(&e);
            self.journal_dirty = true;
            self.push_log(e);
            adopted += 1;
        }
        if let Some(r) = self.recovery.as_mut() {
            r.staged = staged;
        }
        if adopted > 0 {
            self.notices
                .push(format!("adopted {adopted} entr{} from a terminal that had seen them", if adopted == 1 { "y" } else { "ies" }));
        }
        // a terminal that knows the file was saved more recently than we do
        if saved_rev > self.saved_rev && saved_rev <= self.rev {
            self.saved_rev = saved_rev;
            if !disk_hash.is_empty() {
                self.disk_hash = disk_hash;
            }
        }
    }

    fn handle_while_recovering(&mut self, from: ClientId, msg: ClientMsg) -> Vec<Out> {
        match msg {
            ClientMsg::Rejoin { client, rev, recent, in_flight, saved_rev, disk_hash, .. } => {
                self.stage_rejoin(client, recent, saved_rev, disk_hash);
                if let Some(r) = self.recovery.as_mut() {
                    r.rejoined.push(Rejoin { client, rev, in_flight });
                    let done = r.expected.iter().all(|c| *c == client || r.rejoined.iter().any(|x| x.client == *c));
                    if done {
                        return self.finish_recovery();
                    }
                }
                vec![]
            }
            ClientMsg::Bye { client } => {
                self.clients.remove(&client);
                if let Some(r) = self.recovery.as_mut() {
                    r.expected.remove(&client);
                }
                vec![]
            }
            other => {
                if let Some(r) = self.recovery.as_mut() {
                    r.queued.push((from, other));
                }
                vec![]
            }
        }
    }

    /// End the collection window: everything the terminals knew is adopted, so now answer them and carry on.
    fn finish_recovery(&mut self) -> Vec<Out> {
        let Some(r) = self.recovery.take() else { return vec![] };
        if !r.staged.is_empty() {
            self.notices.push(format!(
                "{} entr{} could not be linked to the history and were skipped",
                r.staged.len(),
                if r.staged.len() == 1 { "y" } else { "ies" }
            ));
        }
        let mut out = vec![];
        for rj in r.rejoined {
            out.extend(self.respond_rejoin(rj));
        }
        for (from, msg) in r.queued {
            out.extend(self.handle(from, msg));
        }
        out.push(self.roster());
        out
    }

    /// Bring one rejoined terminal up to date and commit its edit that was still in flight.
    fn respond_rejoin(&mut self, rj: Rejoin) -> Vec<Out> {
        let Rejoin { client, rev: client_rev, in_flight } = rj;
        self.clients.insert(client);
        let mut out = vec![];
        let oldest = self.log.front().map(|e| e.rev).unwrap_or(self.rev + 1);
        if client_rev > self.rev || client_rev + 1 < oldest {
            // cannot bridge the gap from the history we have: start this terminal over from the shared text
            out.extend(self.resync(client, "your copy is too far from the shared text"));
            out.push(self.roster());
            return out;
        }
        out.push(one(
            client,
            ServerMsg::Rejoined { epoch: self.epoch, leader: self.me, rev: self.rev, saved_rev: self.saved_rev, conflict: self.conflict },
        ));
        // everything the terminal is missing, in order (its own confirmed-but-unacknowledged edit arrives here too)
        for e in self.log.iter().filter(|e| e.rev > client_rev) {
            out.push(one(client, ServerMsg::Apply(e.clone())));
        }
        // its edit that was still in flight, unless it is one of the entries just sent
        if let Some(f) = in_flight
            && !self.log.iter().any(|e| e.id == Some(f.id))
        {
            out.extend(self.on_edit(client, f.id, f.base_rev, f.ops));
        }
        out
    }
}

/// Placeholder while `open` builds the core before the real journal is attached.
struct NullJournal;

impl Journal for NullJournal {
    fn append(&mut self, _: &Committed) -> io::Result<()> {
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn reset(&mut self, _: &str, _: u64) -> io::Result<()> {
        Ok(())
    }
    fn load(&self) -> io::Result<Option<JournalContents>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::super::ot::{del, ins};
    use super::super::sim::*;
    use super::*;

    const P: &str = "/work/file.txt";

    fn leader(text: &str) -> (LeaderCore, SimFs, SimJournal) {
        let fs = SimFs::with_file(P, text);
        let j = SimJournal::default();
        let l = LeaderCore::open(1, P.into(), Box::new(fs.clone()), Box::new(j.clone()), 1, 0).unwrap();
        (l, fs, j)
    }

    fn mid(c: u64, s: u64) -> MsgId {
        MsgId { client: c, seq: s }
    }

    fn edit(l: &mut LeaderCore, c: u64, s: u64, base: u64, ops: Vec<Op>) -> Vec<Out> {
        l.handle(c, ClientMsg::Edit { id: mid(c, s), base_rev: base, ops })
    }

    #[test]
    fn open_reads_the_file_and_join_sends_the_text() {
        let (mut l, _, _) = leader("hello\nworld");
        let out = l.join(7);
        let ServerMsg::Welcome { text, rev, saved_rev, clients, .. } = &out[0].msg else { panic!() };
        assert_eq!((text.as_str(), *rev, *saved_rev, clients.clone()), ("hello\nworld", 0, 0, vec![7]));
    }

    #[test]
    fn a_missing_file_is_an_empty_document_and_binary_files_are_refused() {
        let fs = SimFs::default();
        let l = LeaderCore::open(1, "/new.txt".into(), Box::new(fs.clone()), Box::new(SimJournal::default()), 1, 0).unwrap();
        assert_eq!(l.text(), "");
        fs.external_write(std::path::Path::new("/bin"), &[0xff, 0xfe, 0x00]);
        assert!(LeaderCore::open(1, "/bin".into(), Box::new(fs), Box::new(SimJournal::default()), 1, 0).is_err());
    }

    #[test]
    fn an_edit_is_committed_acked_and_broadcast() {
        let (mut l, _, _) = leader("abc");
        l.join(1);
        l.join(2);
        let out = edit(&mut l, 1, 1, 0, vec![ins(3, "d")]);
        assert_eq!(l.text(), "abcd");
        assert!(out.iter().any(|o| o.to == Dest::One(1) && matches!(o.msg, ServerMsg::Ack { rev: 1, .. })));
        assert!(out.iter().any(|o| o.to == Dest::AllExcept(1) && matches!(&o.msg, ServerMsg::Apply(c) if c.rev == 1 && c.origin == 1)));
        assert!(l.is_dirty());
    }

    #[test]
    fn concurrent_edits_are_transformed_and_ordered() {
        let (mut l, _, _) = leader("hello world");
        l.join(1);
        l.join(2);
        edit(&mut l, 1, 1, 0, vec![ins(0, ">> ")]);
        // client 2 made its edit before seeing client 1's
        let out = edit(&mut l, 2, 1, 0, vec![del(6, "world"), ins(6, "there")]);
        assert_eq!(l.text(), ">> hello there");
        let ServerMsg::Apply(c) = &out.iter().find(|o| matches!(o.msg, ServerMsg::Apply(_))).unwrap().msg else { panic!() };
        assert_eq!(c.ops, vec![del(9, "world"), ins(9, "there")], "what the others receive is shifted past the earlier insert");
    }

    #[test]
    fn same_position_inserts_put_the_earlier_commit_first() {
        let (mut l, _, _) = leader("ab");
        l.join(1);
        l.join(2);
        edit(&mut l, 1, 1, 0, vec![ins(1, "X")]);
        edit(&mut l, 2, 1, 0, vec![ins(1, "Y")]);
        assert_eq!(l.text(), "aXYb");
    }

    #[test]
    fn an_edit_that_was_already_committed_is_confirmed_not_applied_twice() {
        let (mut l, _, _) = leader("a");
        l.join(1);
        edit(&mut l, 1, 1, 0, vec![ins(1, "b")]);
        let out = edit(&mut l, 1, 1, 0, vec![ins(1, "b")]);
        assert_eq!(l.text(), "ab");
        assert!(matches!(out[0].msg, ServerMsg::Ack { rev: 1, .. }));
        assert_eq!(l.rev, 1);
    }

    #[test]
    fn invalid_edits_trigger_a_resync_instead_of_corrupting_the_text() {
        let (mut l, _, _) = leader("abc");
        l.join(1);
        for bad in [vec![ins(99, "x")], vec![del(2, "zz")], vec![del(0, "xyz")]] {
            let out = edit(&mut l, 1, 5, 0, bad);
            assert_eq!(l.text(), "abc");
            assert!(out.iter().any(|o| matches!(o.msg, ServerMsg::Welcome { .. })), "client is re-synchronised");
        }
        let out = edit(&mut l, 1, 6, 42, vec![ins(0, "x")]);
        assert!(out.iter().any(|o| matches!(o.msg, ServerMsg::Welcome { .. })), "a revision from the future");
    }

    #[test]
    fn undo_is_shared_across_terminals() {
        let (mut l, _, _) = leader("");
        l.join(1);
        l.join(2);
        edit(&mut l, 1, 1, 0, vec![ins(0, "from-one ")]);
        edit(&mut l, 2, 1, 1, vec![Op::Mark, ins(9, "from-two")]);
        assert_eq!(l.text(), "from-one from-two");
        // terminal 1 presses undo: it undoes terminal 2's latest edit, like VS Code's tabs
        let out = l.handle(1, ClientMsg::Undo { id: mid(1, 2) });
        assert_eq!(l.text(), "from-one ");
        assert!(matches!(&out[0].msg, ServerMsg::Apply(c) if c.kind == Kind::Undo && c.origin == 1));
        l.handle(2, ClientMsg::Undo { id: mid(2, 2) });
        assert_eq!(l.text(), "");
        l.handle(1, ClientMsg::Redo { id: mid(1, 3) });
        assert_eq!(l.text(), "from-one ");
        l.handle(1, ClientMsg::Redo { id: mid(1, 4) });
        assert_eq!(l.text(), "from-one from-two");
    }

    #[test]
    fn typing_runs_are_one_undo_step_and_marks_split_them() {
        let (mut l, _, _) = leader("");
        l.join(1);
        edit(&mut l, 1, 1, 0, vec![ins(0, "a")]);
        edit(&mut l, 1, 2, 1, vec![ins(1, "b")]);
        edit(&mut l, 1, 3, 2, vec![Op::Mark, ins(2, "c")]);
        assert_eq!(l.text(), "abc");
        l.handle(1, ClientMsg::Undo { id: mid(1, 4) });
        assert_eq!(l.text(), "ab", "the mark started a new group");
        l.handle(1, ClientMsg::Undo { id: mid(1, 5) });
        assert_eq!(l.text(), "", "a and b were one typing run");
        let out = l.handle(1, ClientMsg::Undo { id: mid(1, 6) });
        assert!(matches!(&out[0].msg, ServerMsg::Notice { text } if text.contains("oldest")));
    }

    #[test]
    fn a_new_edit_clears_redo_and_undo_survives_other_terminals_edits() {
        let (mut l, _, _) = leader("0123456789");
        l.join(1);
        l.join(2);
        edit(&mut l, 1, 1, 0, vec![Op::Mark, del(2, "23")]);
        edit(&mut l, 2, 1, 1, vec![Op::Mark, ins(0, "XX")]);
        assert_eq!(l.text(), "XX01456789");
        l.handle(1, ClientMsg::Undo { id: mid(1, 2) }); // undoes terminal 2's insert
        assert_eq!(l.text(), "01456789");
        l.handle(1, ClientMsg::Undo { id: mid(1, 3) }); // then terminal 1's delete, still correctly placed
        assert_eq!(l.text(), "0123456789");
        l.handle(1, ClientMsg::Redo { id: mid(1, 4) });
        assert_eq!(l.text(), "01456789");
        let r = l.rev;
        edit(&mut l, 2, 2, r, vec![Op::Mark, ins(0, "!")]);
        let out = l.handle(1, ClientMsg::Redo { id: mid(1, 5) });
        assert!(matches!(&out[0].msg, ServerMsg::Notice { text } if text.contains("nothing to redo")), "a new edit cleared redo");
    }

    #[test]
    fn undo_focus_points_at_the_change() {
        let (mut l, _, _) = leader("hello");
        l.join(1);
        edit(&mut l, 1, 1, 0, vec![Op::Mark, ins(5, " world")]);
        let out = l.handle(1, ClientMsg::Undo { id: mid(1, 2) });
        assert!(matches!(&out[0].msg, ServerMsg::Apply(c) if c.focus == Some(5)));
    }

    #[test]
    fn save_writes_atomically_once_and_marks_everyone_clean() {
        let (mut l, fs, j) = leader("a");
        l.join(1);
        l.join(2);
        edit(&mut l, 1, 1, 0, vec![ins(1, "b")]);
        edit(&mut l, 2, 1, 1, vec![ins(2, "c")]);
        let out = l.handle(2, ClientMsg::Save { id: mid(2, 2), force: false });
        assert_eq!(fs.read_string(P).as_deref(), Some("abc"), "edits from both terminals are in the file");
        assert!(matches!(&out[0], Out { to: Dest::All, msg: ServerMsg::Saved { rev: 2, .. } }));
        assert!(!l.is_dirty());
        assert_eq!(j.0.lock().unwrap().durable.len(), 0, "the journal restarts after a save");
        assert_eq!(fs.write_calls(), 1);
        let out = l.handle(1, ClientMsg::Save { id: mid(1, 3), force: false });
        assert!(matches!(&out[0].msg, ServerMsg::Saved { rev: 2, .. }), "saving again just reconfirms");
    }

    #[test]
    fn a_failed_save_leaves_the_file_and_the_dirty_state_alone() {
        let (mut l, fs, _) = leader("a");
        l.join(1);
        edit(&mut l, 1, 1, 0, vec![ins(1, "b")]);
        fs.0.lock().unwrap().fail_writes = true;
        let out = l.handle(1, ClientMsg::Save { id: mid(1, 2), force: false });
        assert!(matches!(&out[0].msg, ServerMsg::SaveFailed { .. }));
        assert_eq!(fs.read_string(P).as_deref(), Some("a"));
        assert!(l.is_dirty());
        fs.0.lock().unwrap().fail_writes = false;
        l.handle(1, ClientMsg::Save { id: mid(1, 3), force: false });
        assert_eq!(fs.read_string(P).as_deref(), Some("ab"));
    }

    #[test]
    fn a_crash_during_save_leaves_the_old_complete_file() {
        let (mut l, fs, _) = leader("old content");
        l.join(1);
        edit(&mut l, 1, 1, 0, vec![ins(0, "new ")]);
        fs.0.lock().unwrap().crash_on_write = Some(1);
        l.handle(1, ClientMsg::Save { id: mid(1, 2), force: false });
        assert_eq!(fs.read_string(P).as_deref(), Some("old content"), "never half-written");
        assert!(l.is_dirty());
    }

    #[test]
    fn save_refuses_to_overwrite_a_file_changed_elsewhere() {
        let (mut l, fs, _) = leader("a");
        l.join(1);
        edit(&mut l, 1, 1, 0, vec![ins(1, "b")]);
        fs.external_write(std::path::Path::new(P), b"someone else wrote this");
        let out = l.handle(1, ClientMsg::Save { id: mid(1, 2), force: false });
        assert!(matches!(&out[0].msg, ServerMsg::SaveRefused { reason, .. } if reason.contains(":w!")));
        assert_eq!(fs.read_string(P).as_deref(), Some("someone else wrote this"));
        let out = l.handle(1, ClientMsg::Save { id: mid(1, 3), force: true });
        assert!(matches!(&out[0].msg, ServerMsg::Saved { .. }));
        assert_eq!(fs.read_string(P).as_deref(), Some("ab"));
    }

    #[test]
    fn an_external_change_is_followed_when_clean_and_flagged_when_dirty() {
        let (mut l, fs, _) = leader("one\ntwo\nthree");
        l.join(1);
        fs.external_write(std::path::Path::new(P), b"one\n2\nthree");
        let out = l.tick(1000);
        assert_eq!(l.text(), "one\n2\nthree");
        assert!(
            out.iter()
                .any(|o| matches!(&o.msg, ServerMsg::Apply(c) if c.kind == Kind::External && c.ops == vec![del(4, "two"), ins(4, "2")]))
        );
        assert!(out.iter().any(|o| matches!(o.msg, ServerMsg::Reloaded { .. })));
        assert!(!l.is_dirty());
        // now with unsaved edits
        let r = l.rev;
        edit(&mut l, 1, 1, r, vec![ins(0, "mine ")]);
        fs.external_write(std::path::Path::new(P), b"completely different");
        let out = l.tick(2000);
        assert_eq!(l.text(), "mine one\n2\nthree", "unsaved work is never replaced");
        assert!(out.iter().any(|o| matches!(o.msg, ServerMsg::Conflict { .. })));
        assert!(l.conflict);
    }

    #[test]
    fn touching_the_file_without_changing_it_does_nothing() {
        let (mut l, fs, _) = leader("same");
        fs.external_write(std::path::Path::new(P), b"same");
        assert!(l.tick(1000).is_empty());
        assert_eq!(l.rev, 0);
    }

    #[test]
    fn journal_is_flushed_on_a_timer_and_recovers_unsaved_edits_after_everything_died() {
        let (mut l, fs, j) = leader("base");
        l.join(1);
        edit(&mut l, 1, 1, 0, vec![ins(4, " one")]);
        edit(&mut l, 1, 2, 1, vec![ins(8, " two")]);
        assert_eq!(j.durable_len(), 0, "appended but not flushed yet");
        l.tick(500);
        assert_eq!(j.durable_len(), 0);
        l.tick(1000);
        assert_eq!(j.durable_len(), 2, "flushed after a second");
        edit(&mut l, 1, 3, 2, vec![ins(12, " three")]);
        j.crash(); // power cut: the unflushed third edit is lost, the first two survive
        drop(l);
        let mut l2 = LeaderCore::open(1, P.into(), Box::new(fs.clone()), Box::new(j.clone()), 2, 0).unwrap();
        assert_eq!(l2.text(), "base one two");
        assert_eq!(l2.recovered, Some(2));
        assert!(l2.is_dirty(), "the recovered work is still unsaved");
        assert!(l2.take_notices().iter().any(|n| n.contains("recovered 2")));
        assert_eq!(fs.read_string(P).as_deref(), Some("base"), "the file itself was never touched");
    }

    #[test]
    fn the_journal_is_compacted_to_the_net_change_and_still_recovers_the_same_text() {
        let (mut l, fs, j) = leader("base text\n");
        l.join(1);
        // many small edits that mostly cancel out
        let mut rev = 0;
        for i in 0..400u64 {
            let text = "y".repeat(5000);
            edit(&mut l, 1, i * 2 + 1, rev, vec![ins(0, &text)]);
            rev += 1;
            edit(&mut l, 1, i * 2 + 2, rev, vec![del(0, &text)]);
            rev += 1;
        }
        edit(&mut l, 1, 9999, rev, vec![ins(10, "FINAL")]);
        l.flush_journal(0);
        assert!(j.durable_len() > 100, "before compaction the journal holds every edit ({})", j.durable_len());
        l.tick(5000);
        assert!(j.durable_len() <= 1, "after compaction a single net change remains ({})", j.durable_len());
        let expect = l.text();
        assert_eq!(expect, "base text\nFINAL");
        drop(l);
        let l2 = LeaderCore::open(1, P.into(), Box::new(fs), Box::new(j), 2, 0).unwrap();
        assert_eq!(l2.text(), expect, "the compacted journal recovers exactly the same text");
        assert!(l2.is_dirty());
    }

    #[test]
    fn a_journal_for_a_different_version_of_the_file_is_not_replayed() {
        let (mut l, fs, j) = leader("v1");
        l.join(1);
        edit(&mut l, 1, 1, 0, vec![ins(2, "!")]);
        l.flush_journal(0);
        drop(l);
        fs.external_write(std::path::Path::new(P), b"v2 changed meanwhile");
        let mut l2 = LeaderCore::open(1, P.into(), Box::new(fs), Box::new(j), 2, 0).unwrap();
        assert_eq!(l2.text(), "v2 changed meanwhile");
        assert!(l2.recovered.is_none());
        assert!(l2.take_notices().iter().any(|n| n.contains("changed since")));
    }

    #[test]
    fn rejoin_catches_a_terminal_up_and_resends_its_unconfirmed_edit_once() {
        // old leader committed revisions 1..2; terminal 2 saw both, terminal 3 only saw 1 and has an edit in flight
        let (mut old, fs, j) = leader("abc");
        old.join(2);
        old.join(3);
        edit(&mut old, 2, 1, 0, vec![ins(3, "D")]);
        edit(&mut old, 2, 2, 1, vec![ins(4, "E")]);
        let recent: Vec<Committed> = old.log.iter().cloned().collect();
        let shadow2 = old.doc().clone();
        drop(old); // the leader dies
        // terminal 2 becomes the new leader from its own replica
        let mut l = LeaderCore::resume(
            2,
            BTreeSet::new(),
            P.into(),
            Box::new(fs),
            Box::new(j),
            2,
            2,
            shadow2,
            recent.clone(),
            0,
            "h".into(),
            false,
            0,
        );
        l.join(2);
        // terminal 3 rejoins: it is at rev 1 with an edit of its own in flight (based on rev 1)
        let out = l.handle(
            3,
            ClientMsg::Rejoin {
                client: 3,
                epoch: 1,
                rev: 1,
                recent: recent[..1].to_vec(),
                in_flight: Some(InFlight { id: mid(3, 1), base_rev: 1, ops: vec![ins(0, "Z")] }),
                saved_rev: 0,
                disk_hash: "h".into(),
            },
        );
        assert!(matches!(out[0].msg, ServerMsg::Rejoined { rev: 2, .. }));
        assert!(matches!(&out[1].msg, ServerMsg::Apply(c) if c.rev == 2), "it is sent what it missed");
        assert!(out.iter().any(|o| matches!(o.msg, ServerMsg::Ack { rev: 3, .. })), "then its own edit is committed");
        assert_eq!(l.text(), "ZabcDE");
        // sending the same rejoin again must not apply the edit twice
        let again = l.handle(
            3,
            ClientMsg::Rejoin {
                client: 3,
                epoch: 1,
                rev: 1,
                recent: vec![],
                in_flight: Some(InFlight { id: mid(3, 1), base_rev: 1, ops: vec![ins(0, "Z")] }),
                saved_rev: 0,
                disk_hash: "h".into(),
            },
        );
        assert_eq!(l.text(), "ZabcDE");
        assert!(
            again.iter().any(|o| matches!(&o.msg, ServerMsg::Apply(c) if c.id == Some(mid(3, 1)))),
            "the already-committed edit is replayed as an entry"
        );
    }

    #[test]
    fn resume_adopts_edits_that_only_the_journal_remembers() {
        let (mut old, fs, j) = leader("abc");
        old.join(1);
        edit(&mut old, 1, 1, 0, vec![ins(3, "X")]);
        old.flush_journal(0);
        edit(&mut old, 1, 2, 1, vec![ins(4, "Y")]);
        old.flush_journal(0);
        let shadow_of_a_terminal_that_only_saw_rev_1 = {
            let mut r = old.doc().clone();
            apply_rope(&mut r, &del(4, "Y"));
            r
        };
        let e1 = old.log.front().cloned().unwrap();
        drop(old);
        let mut l = LeaderCore::resume(
            2,
            BTreeSet::new(),
            P.into(),
            Box::new(fs),
            Box::new(j),
            2,
            1,
            shadow_of_a_terminal_that_only_saw_rev_1,
            vec![e1],
            0,
            "h".into(),
            false,
            0,
        );
        assert_eq!(l.text(), "abcXY", "revision 2 came back from the journal");
        assert_eq!(l.rev, 2);
        assert!(l.take_notices().iter().any(|n| n.contains("journal")));
    }

    #[test]
    fn resume_notices_that_the_dead_leader_had_just_saved() {
        let (mut old, fs, j) = leader("a");
        old.join(1);
        edit(&mut old, 1, 1, 0, vec![ins(1, "b")]);
        let shadow = old.doc().clone();
        let recent: Vec<Committed> = old.log.iter().cloned().collect();
        old.handle(1, ClientMsg::Save { id: mid(1, 2), force: false });
        drop(old); // died right after writing, before anyone heard "Saved"
        let mut l = LeaderCore::resume(
            2,
            BTreeSet::new(),
            P.into(),
            Box::new(fs),
            Box::new(j),
            2,
            1,
            shadow,
            recent,
            0,
            hash_bytes(b"a"),
            false,
            0,
        );
        assert!(l.is_dirty());
        let out = l.tick(1000);
        assert!(out.iter().any(|o| matches!(o.msg, ServerMsg::Saved { rev: 1, .. })));
        assert!(!l.is_dirty());
    }
}
