//! Text operations and operational transformation.
//!
//! Positions are char indices. `Del` carries the deleted text so every operation can be inverted (undo) and
//! so overlapping deletions can be trimmed exactly. `Mark` is an undo-group boundary: it does nothing to the text
//! and passes through every transformation untouched.
//!
//! The core property (called TP1) is: for operations `a` and `b` made on the same document,
//! `apply(a); apply(b')` and `apply(b); apply(a')` give the same text, where `(a', b') = transform(a, b)`.
//! `tests` below and `tests/prop_sync.rs` check it on random inputs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    Ins { pos: usize, text: String },
    Del { pos: usize, text: String },
    Mark,
}

pub fn ins(pos: usize, text: &str) -> Op {
    Op::Ins { pos, text: text.to_string() }
}

pub fn del(pos: usize, text: &str) -> Op {
    Op::Del { pos, text: text.to_string() }
}

fn clen(s: &str) -> usize {
    s.chars().count()
}

fn csub(s: &str, from: usize, to: usize) -> String {
    s.chars().skip(from).take(to.saturating_sub(from)).collect()
}

impl Op {
    /// The operation that undoes this one (applied right after it).
    pub fn inverse(&self) -> Op {
        match self {
            Op::Ins { pos, text } => Op::Del { pos: *pos, text: text.clone() },
            Op::Del { pos, text } => Op::Ins { pos: *pos, text: text.clone() },
            Op::Mark => Op::Mark,
        }
    }

    pub fn is_noop(&self) -> bool {
        match self {
            Op::Ins { text, .. } | Op::Del { text, .. } => text.is_empty(),
            Op::Mark => false,
        }
    }

    /// Number of chars this op adds (positive) or removes (negative).
    pub fn delta(&self) -> isize {
        match self {
            Op::Ins { text, .. } => clen(text) as isize,
            Op::Del { text, .. } => -(clen(text) as isize),
            Op::Mark => 0,
        }
    }
}

/// Apply to a plain string (tests and the leader's snapshot). Out-of-range positions are clamped.
pub fn apply_str(doc: &mut String, op: &Op) {
    match op {
        Op::Ins { pos, text } => {
            let at = doc.char_indices().nth(*pos).map(|(i, _)| i).unwrap_or(doc.len());
            doc.insert_str(at, text);
        }
        Op::Del { pos, text } => {
            let start = doc.char_indices().nth(*pos).map(|(i, _)| i).unwrap_or(doc.len());
            let end = doc.char_indices().nth(*pos + clen(text)).map(|(i, _)| i).unwrap_or(doc.len());
            doc.replace_range(start..end, "");
        }
        Op::Mark => {}
    }
}

pub fn apply_all(doc: &mut String, ops: &[Op]) {
    for o in ops {
        apply_str(doc, o);
    }
}

/// Transform a cursor-like position through `op`. A remote insert exactly at `pos` does not push it
/// unless `stick_right` (used for positions that should stay after inserted text).
pub fn transform_pos(pos: usize, op: &Op, stick_right: bool) -> usize {
    match op {
        Op::Ins { pos: p, text } => {
            if *p < pos || (*p == pos && stick_right) {
                pos + clen(text)
            } else {
                pos
            }
        }
        Op::Del { pos: p, text } => {
            let l = clen(text);
            if pos > *p { (*p).max(pos.saturating_sub(l)) } else { pos }
        }
        Op::Mark => pos,
    }
}

/// Transform `a` and `b` (both made on the same document) against each other.
/// Returns `(a', b')`: `a'` applies after `b`, `b'` applies after `a`. When both insert at the same
/// position, `a_wins` decides whose text comes first.
pub fn transform(a: &Op, b: &Op, a_wins: bool) -> (Vec<Op>, Vec<Op>) {
    use Op::*;
    match (a, b) {
        (Mark, _) | (_, Mark) => (vec![a.clone()], vec![b.clone()]),
        (Ins { pos: ap, text: at }, Ins { pos: bp, text: bt }) => {
            if ap < bp || (ap == bp && a_wins) {
                (vec![a.clone()], vec![Ins { pos: bp + clen(at), text: bt.clone() }])
            } else {
                (vec![Ins { pos: ap + clen(bt), text: at.clone() }], vec![b.clone()])
            }
        }
        (Ins { .. }, Del { .. }) => ins_vs_del(a, b),
        (Del { .. }, Ins { .. }) => {
            let (b2, a2) = ins_vs_del(b, a);
            (a2, b2)
        }
        (Del { pos: ap, text: at }, Del { pos: bp, text: bt }) => {
            let (al, bl) = (clen(at), clen(bt));
            let (a_end, b_end) = (ap + al, bp + bl);
            if a_end <= *bp {
                // a entirely before b
                (vec![a.clone()], vec![Del { pos: bp - al, text: bt.clone() }])
            } else if b_end <= *ap {
                (vec![Del { pos: ap - bl, text: at.clone() }], vec![b.clone()])
            } else {
                // overlap: each side keeps only what the other did not already delete
                let (s, e) = ((*ap).max(*bp), a_end.min(b_end));
                let rest = |own_pos: usize, own_text: &str, own_end: usize| -> Vec<Op> {
                    let before = csub(own_text, 0, s.saturating_sub(own_pos));
                    let after = csub(own_text, e - own_pos, own_end - own_pos);
                    let t = before + &after;
                    if t.is_empty() { vec![] } else { vec![Del { pos: own_pos.min(*ap).min(*bp), text: t }] }
                };
                (rest(*ap, at, a_end), rest(*bp, bt, b_end))
            }
        }
    }
}

/// `ins` happened at the same time as `del`. Returns (ins', del').
fn ins_vs_del(i: &Op, d: &Op) -> (Vec<Op>, Vec<Op>) {
    let (Op::Ins { pos: ip, text: it }, Op::Del { pos: dp, text: dt }) = (i, d) else { unreachable!() };
    let (il, dl) = (clen(it), clen(dt));
    if *ip <= *dp {
        // insertion before (or at the start of) the deleted range
        (vec![i.clone()], vec![Op::Del { pos: dp + il, text: dt.clone() }])
    } else if *ip >= dp + dl {
        (vec![Op::Ins { pos: ip - dl, text: it.clone() }], vec![d.clone()])
    } else {
        // insertion lands inside the deleted range: the text survives, the deletion splits around it
        let k = ip - dp;
        let first = csub(dt, 0, k);
        let second = csub(dt, k, dl);
        (vec![Op::Ins { pos: *dp, text: it.clone() }], vec![Op::Del { pos: *dp, text: first }, Op::Del { pos: dp + il, text: second }])
    }
}

/// Transform two sequences of operations made on the same document.
pub fn transform_seq(a: &[Op], b: &[Op], a_wins: bool) -> (Vec<Op>, Vec<Op>) {
    let (a2, b2) = tseq(a, b, a_wins, 0);
    (drop_noops(a2), drop_noops(b2))
}

fn drop_noops(v: Vec<Op>) -> Vec<Op> {
    v.into_iter().filter(|o| !o.is_noop()).collect()
}

fn tseq(a: &[Op], b: &[Op], a_wins: bool, depth: usize) -> (Vec<Op>, Vec<Op>) {
    debug_assert!(depth < 20_000, "transform recursion too deep");
    if a.is_empty() {
        return (vec![], b.to_vec());
    }
    if b.is_empty() {
        return (a.to_vec(), vec![]);
    }
    if a.len() == 1 && b.len() == 1 {
        return transform(&a[0], &b[0], a_wins);
    }
    if a.len() > 1 {
        let (a0, rest) = a.split_at(1);
        let (a0p, b1) = tseq(a0, b, a_wins, depth + 1);
        let (restp, b2) = tseq(rest, &b1, a_wins, depth + 1);
        let mut out = a0p;
        out.extend(restp);
        (out, b2)
    } else {
        let (b0, rest) = b.split_at(1);
        let (a1, b0p) = tseq(a, b0, a_wins, depth + 1);
        let (a2, restp) = tseq(&a1, rest, a_wins, depth + 1);
        let mut out = b0p;
        out.extend(restp);
        (a2, out)
    }
}

/// Merge neighbouring operations that are one typing run (`a`,`b` -> `ab`; repeated Backspace/Delete).
pub fn compact(ops: Vec<Op>) -> Vec<Op> {
    let mut out: Vec<Op> = Vec::with_capacity(ops.len());
    for op in ops {
        if let (Some(last), true) = (out.last_mut(), !op.is_noop()) {
            match (&mut *last, &op) {
                (Op::Ins { pos: lp, text: lt }, Op::Ins { pos, text }) if *pos == *lp + clen(lt) => {
                    lt.push_str(text);
                    continue;
                }
                // backspacing: delete just before the previous deletion
                (Op::Del { pos: lp, text: lt }, Op::Del { pos, text }) if *pos + clen(text) == *lp => {
                    *lp = *pos;
                    *lt = format!("{text}{lt}");
                    continue;
                }
                // forward delete: delete at the same position again
                (Op::Del { pos: lp, text: lt }, Op::Del { pos, text }) if *pos == *lp => {
                    lt.push_str(text);
                    continue;
                }
                _ => {}
            }
        }
        if !op.is_noop() {
            out.push(op);
        }
    }
    out
}

/// The smallest `[Del, Ins]` that turns `old` into `new` (common prefix and suffix trimmed). Used to turn an
/// external file change into an operation without rewriting the whole buffer.
pub fn diff_ops(old: &str, new: &str) -> Vec<Op> {
    if old == new {
        return vec![];
    }
    let (o, n): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let prefix = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let max_suffix = o.len().min(n.len()) - prefix;
    let suffix = o.iter().rev().zip(n.iter().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
    let removed: String = o[prefix..o.len() - suffix].iter().collect();
    let added: String = n[prefix..n.len() - suffix].iter().collect();
    let mut ops = vec![];
    if !removed.is_empty() {
        ops.push(Op::Del { pos: prefix, text: removed });
    }
    if !added.is_empty() {
        ops.push(Op::Ins { pos: prefix, text: added });
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(doc: &str, ops: &[Op]) -> String {
        let mut d = doc.to_string();
        apply_all(&mut d, ops);
        d
    }

    /// TP1 for one pair, both tie-break orders.
    fn tp1(doc: &str, a: &[Op], b: &[Op]) {
        for a_wins in [true, false] {
            let (a2, b2) = transform_seq(a, b, a_wins);
            let left = run(&run(doc, a), &b2);
            let right = run(&run(doc, b), &a2);
            assert_eq!(left, right, "doc {doc:?}, a {a:?}, b {b:?}, a_wins {a_wins}");
        }
    }

    #[test]
    fn insert_insert_orders_by_position_and_tie_break() {
        let (a2, b2) = transform(&ins(2, "AA"), &ins(5, "B"), true);
        assert_eq!((a2, b2), (vec![ins(2, "AA")], vec![ins(7, "B")]));
        let (a2, b2) = transform(&ins(3, "A"), &ins(3, "B"), true);
        assert_eq!((a2, b2), (vec![ins(3, "A")], vec![ins(4, "B")]), "a wins: a's text first");
        let (a2, b2) = transform(&ins(3, "A"), &ins(3, "B"), false);
        assert_eq!((a2, b2), (vec![ins(4, "A")], vec![ins(3, "B")]), "b wins: b's text first");
        tp1("0123456789", &[ins(3, "A")], &[ins(3, "B")]);
    }

    #[test]
    fn insert_delete_all_geometries() {
        let doc = "0123456789";
        for ip in 0..=10 {
            for dp in 0..10 {
                for dl in 1..=(10 - dp) {
                    let d = del(dp, &doc[dp..dp + dl]);
                    tp1(doc, &[ins(ip, "XY")], &[d]);
                }
            }
        }
    }

    #[test]
    fn insertion_inside_a_deleted_range_survives() {
        let doc = "abcdef";
        let (a2, b2) = transform(&ins(3, "XY"), &del(1, "bcde"), true);
        assert_eq!(run(&run(doc, &[ins(3, "XY")]), &b2), "aXYf");
        assert_eq!(run(&run(doc, &[del(1, "bcde")]), &a2), "aXYf");
    }

    #[test]
    fn delete_delete_all_overlaps() {
        let doc = "0123456789";
        for ap in 0..10 {
            for al in 1..=(10 - ap) {
                for bp in 0..10 {
                    for bl in 1..=(10 - bp) {
                        tp1(doc, &[del(ap, &doc[ap..ap + al])], &[del(bp, &doc[bp..bp + bl])]);
                    }
                }
            }
        }
    }

    #[test]
    fn identical_deletions_cancel() {
        let (a2, b2) = transform(&del(2, "cd"), &del(2, "cd"), true);
        assert!(a2.is_empty() && b2.is_empty());
    }

    #[test]
    fn marks_pass_through_untouched() {
        let (a2, b2) = transform(&Op::Mark, &ins(1, "x"), true);
        assert_eq!((a2, b2), (vec![Op::Mark], vec![ins(1, "x")]));
        let (a2, _) = transform_seq(&[ins(0, "a"), Op::Mark, ins(1, "b")], &[ins(0, "Z")], false);
        assert!(a2.contains(&Op::Mark));
    }

    #[test]
    fn sequences_converge() {
        let doc = "hello world, this is a test";
        let a = vec![ins(5, ","), del(7, "w"), ins(7, "W"), ins(20, "!!")];
        let b = vec![del(0, "hello "), ins(0, "Oh "), del(10, "this "), ins(3, "wow")];
        tp1(doc, &a, &b);
        tp1(doc, &b, &a);
    }

    #[test]
    fn unicode_is_counted_in_chars() {
        tp1("é€😀日本", &[ins(2, "→")], &[del(1, "€😀")]);
        tp1("é€😀日本", &[del(0, "é€")], &[del(1, "€😀日")]);
        assert_eq!(run("é😀x", &[ins(1, "ü")]), "éü😀x");
        assert_eq!(run("é😀x", &[del(1, "😀")]), "éx");
    }

    #[test]
    fn inverse_undoes() {
        for (doc, op) in [("abc", ins(1, "XY")), ("abcdef", del(2, "cd")), ("é😀", ins(1, "日"))] {
            let after = run(doc, std::slice::from_ref(&op));
            assert_eq!(run(&after, &[op.inverse()]), doc);
        }
    }

    #[test]
    fn positions_follow_remote_edits() {
        assert_eq!(transform_pos(5, &ins(2, "ab"), false), 7);
        assert_eq!(transform_pos(5, &ins(5, "ab"), false), 5, "an insert at the cursor leaves it before the new text");
        assert_eq!(transform_pos(5, &ins(5, "ab"), true), 7);
        assert_eq!(transform_pos(5, &ins(8, "ab"), false), 5);
        assert_eq!(transform_pos(5, &del(1, "ab"), false), 3);
        assert_eq!(transform_pos(5, &del(4, "abc"), false), 4, "inside a deleted range: collapses to its start");
        assert_eq!(transform_pos(2, &del(4, "abc"), false), 2);
    }

    #[test]
    fn compact_merges_typing_and_deleting_runs() {
        assert_eq!(compact(vec![ins(3, "a"), ins(4, "b"), ins(5, "c")]), vec![ins(3, "abc")]);
        assert_eq!(compact(vec![ins(3, "a"), ins(9, "b")]), vec![ins(3, "a"), ins(9, "b")], "not adjacent");
        assert_eq!(compact(vec![del(5, "c"), del(4, "b"), del(3, "a")]), vec![del(3, "abc")], "repeated backspace");
        assert_eq!(compact(vec![del(3, "a"), del(3, "b"), del(3, "c")]), vec![del(3, "abc")], "repeated delete");
        assert_eq!(compact(vec![ins(0, "a"), Op::Mark, ins(1, "b")]), vec![ins(0, "a"), Op::Mark, ins(1, "b")], "marks separate runs");
        assert_eq!(compact(vec![ins(0, ""), del(0, "")]), vec![]);
        let ops = vec![ins(0, "x"), ins(1, "y"), del(0, "x")];
        assert_eq!(run("abc", &compact(ops.clone())), run("abc", &ops));
    }

    #[test]
    fn diff_ops_trims_common_prefix_and_suffix() {
        assert_eq!(diff_ops("abcdef", "abXYef"), vec![del(2, "cd"), ins(2, "XY")]);
        assert_eq!(diff_ops("abc", "abc"), vec![]);
        assert_eq!(diff_ops("", "new"), vec![ins(0, "new")]);
        assert_eq!(diff_ops("old", ""), vec![del(0, "old")]);
        assert_eq!(diff_ops("aaa", "aa"), vec![del(2, "a")], "the prefix is kept, so the surplus is at the end");
        for (a, b) in [("hello world", "hello brave new world"), ("é€😀", "é😀"), ("x", "y"), ("abab", "ab")] {
            assert_eq!(run(a, &diff_ops(a, b)), b);
        }
    }
}
