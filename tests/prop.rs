//! Property and fuzz tests. Short by default (proptest's 256 cases); the nightly CI job sets
//! `PROPTEST_CASES=20000` for a long run. Failing inputs are shrunk and saved under `tests/prop.proptest-regressions`.

use kuro::buffer::Buffer;
use kuro::find::{Finder, apply_replacements};
use kuro::input::{InputEvent, InputParser};
use kuro::suggest::suggest;
use kuro::syntax::{self, Kind};
use kuro::table::{self, Column, TableOpts, TableState};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

fn is_input_event_key_or_mouse(e: &InputEvent) -> bool {
    matches!(e, InputEvent::Key(_) | InputEvent::Mouse(_))
}

/// Complete sequences the parser must understand.
const CORPUS: &[&[u8]] = &[
    b"a",
    "é".as_bytes(),
    "😀".as_bytes(),
    b"\r",
    b"\t",
    b"\x1b[A",
    b"\x1b[1;5C",
    b"\x1b[3~",
    b"\x1b[15~",
    b"\x1bOP",
    b"\x1b[13;5u",
    b"\x1b[9;2u",
    b"\x1b[27u",
    b"\x1b[97;3u",
    b"\x1b[<0;10;5M",
    b"\x1b[<65;1;1M",
    b"\x1b[<16;4;4m",
    b"\x1bc",
    b"\x1b[200~paste me\x1b[201~",
    b"\x1b[?62;c",
];

fn feed_chunks(chunks: &[&[u8]]) -> Vec<InputEvent> {
    let mut p = InputParser::new();
    let mut out = vec![];
    for c in chunks {
        out.extend(p.feed(c));
    }
    out.extend(p.flush_timeout());
    out
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/prop.proptest-regressions"))),
        ..ProptestConfig::default()
    })]

    // ---------------------------------------------------------------- input parser

    #[test]
    fn random_bytes_never_panic_and_flush_leaves_a_clean_parser(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let mut p = InputParser::new();
        for chunk in bytes.chunks(7) {
            let _ = p.feed(chunk);
        }
        let _ = p.flush_timeout();
        prop_assert!(!p.has_pending());
        // and it still works afterwards
        let evs = p.feed(b"z");
        prop_assert_eq!(evs.len(), 1);
    }

    #[test]
    fn splitting_a_stream_anywhere_does_not_change_the_events(
        picks in proptest::collection::vec(0..CORPUS.len(), 1..8),
        cuts in proptest::collection::vec(any::<prop::sample::Index>(), 0..5),
    ) {
        let stream: Vec<u8> = picks.iter().flat_map(|i| CORPUS[*i].iter().copied()).collect();
        let whole = feed_chunks(&[&stream]);
        let mut points: Vec<usize> = cuts.iter().map(|c| c.index(stream.len() + 1)).collect();
        points.sort_unstable();
        let mut chunks: Vec<&[u8]> = vec![];
        let mut prev = 0;
        for p in points {
            chunks.push(&stream[prev..p]);
            prev = p;
        }
        chunks.push(&stream[prev..]);
        prop_assert_eq!(feed_chunks(&chunks), whole);
    }

    #[test]
    fn a_strict_prefix_of_a_sequence_never_yields_a_key_or_mouse_event(
        pick in any::<prop::sample::Index>(),
        cut in any::<prop::sample::Index>(),
    ) {
        // only sequences that have a strict, non-empty prefix
        let multi: Vec<&[u8]> = CORPUS.iter().copied().filter(|s| s.len() > 1).collect();
        let seq = multi[pick.index(multi.len())];
        let n = 1 + cut.index(seq.len() - 1); // 1..len-1
        let mut p = InputParser::new();
        let evs = p.feed(&seq[..n]);
        prop_assert!(!evs.iter().any(is_input_event_key_or_mouse), "{:?} -> {:?}", &seq[..n], evs);
        let after = p.flush_timeout();
        let leaked: Vec<_> = after.iter().filter(|e| is_input_event_key_or_mouse(e)).collect();
        // the only key a timeout may produce is a lone ESC
        let only_lone_esc = &seq[..n] == b"\x1b";
        prop_assert!(leaked.is_empty() || only_lone_esc, "{:?} -> {:?}", &seq[..n], leaked);
    }

    #[test]
    fn mouse_coordinates_are_always_within_what_was_sent(b in 0u32..200, x in 1u16..2000, y in 1u16..2000, release in any::<bool>()) {
        let seq = format!("\x1b[<{b};{x};{y}{}", if release { 'm' } else { 'M' });
        let mut p = InputParser::new();
        for e in p.feed(seq.as_bytes()) {
            if let InputEvent::Mouse(m) = e {
                prop_assert_eq!((m.column, m.row), (x - 1, y - 1));
            }
        }
    }

    // ---------------------------------------------------------------- syntax tokenizer

    #[test]
    fn tokenizer_never_panics_and_yields_one_kind_per_char(src in "\\PC{0,200}") {
        let lines: Vec<String> = src.split('\n').map(String::from).collect();
        for spec in syntax::all_modes() {
            let kinds = syntax::highlight(spec, &lines);
            prop_assert_eq!(kinds.len(), lines.len());
            for (l, k) in lines.iter().zip(&kinds) {
                prop_assert_eq!(l.chars().count(), k.len(), "{}: {:?}", spec.name, l);
            }
            // deterministic
            prop_assert_eq!(&kinds, &syntax::highlight(spec, &lines));
        }
    }

    #[test]
    fn tokenizer_survives_bracket_and_quote_heavy_input(src in "[\\[\\](){}\"'`/*#a-z0-9 =&|<>\\n-]{0,300}") {
        let lines: Vec<String> = src.split('\n').map(String::from).collect();
        for spec in syntax::all_modes() {
            let kinds = syntax::highlight(spec, &lines);
            for (l, k) in lines.iter().zip(&kinds) {
                for (c, kind) in l.chars().zip(k) {
                    match kind {
                        Kind::Bracket(_) | Kind::BracketError => prop_assert!("()[]{}".contains(c), "{c:?} flagged as a bracket"),
                        Kind::Operator => prop_assert!(!c.is_alphanumeric(), "{c:?} flagged as an operator"),
                        _ => {}
                    }
                }
            }
        }
    }

    #[test]
    fn bracket_depth_never_exceeds_the_number_of_openers(src in "[()\\[\\]{}a ]{0,100}") {
        let lines = vec![src.clone()];
        let openers = src.chars().filter(|c| "([{".contains(*c)).count();
        for k in &syntax::highlight(&syntax::GO, &lines)[0] {
            if let Kind::Bracket(d) = k {
                prop_assert!((*d as usize) < openers.max(1));
            }
        }
    }

    // ---------------------------------------------------------------- find

    #[test]
    fn find_matches_are_sorted_disjoint_in_range_and_equal_the_query(
        text in "[abAB \\n]{0,80}",
        query in "[abAB]{1,3}",
        case in any::<bool>(),
    ) {
        let f = Finder { query: query.clone(), case_sensitive: case, whole_word: false, regex: false };
        let ms = f.find_all(&text).unwrap();
        let chars: Vec<char> = text.chars().collect();
        let mut last_end = 0;
        for m in &ms {
            prop_assert!(m.start >= last_end && m.end <= chars.len() && m.start < m.end);
            last_end = m.end;
            let got: String = chars[m.clone()].iter().collect();
            if case {
                prop_assert_eq!(&got, &query);
            } else {
                prop_assert_eq!(got.to_lowercase(), query.to_lowercase());
            }
        }
        // naive count of non-overlapping matches
        let hay = if case { text.clone() } else { text.to_lowercase() };
        let needle = if case { query.clone() } else { query.to_lowercase() };
        prop_assert_eq!(ms.len(), hay.matches(needle.as_str()).count());
    }

    #[test]
    fn replacing_with_the_matched_text_is_a_no_op_and_counts_agree(text in "[abc \\n]{0,60}", q in "[abc]{1,2}") {
        let f = Finder::new(&q);
        let n = f.count(&text).unwrap();
        let (out, k) = f.replace_all(&text, &q).unwrap();
        prop_assert_eq!(k, n);
        prop_assert_eq!(out.to_lowercase(), text.to_lowercase().replace("\r", ""));
    }

    #[test]
    fn arbitrary_regexes_never_panic(text in "\\PC{0,60}", re in "\\PC{0,12}") {
        let f = Finder { query: re, case_sensitive: false, whole_word: true, regex: true };
        let _ = f.find_all(&text);
        let _ = f.replace_all(&text, "$1x");
    }

    #[test]
    fn apply_replacements_matches_a_naive_implementation(text in "[ab]{0,30}") {
        let f = Finder::new("a");
        let edits = f.replacements(&text, "XY").unwrap();
        prop_assert_eq!(apply_replacements(&text, &edits), text.replace('a', "XY"));
    }

    // ---------------------------------------------------------------- buffer

    #[test]
    fn undoing_every_edit_restores_the_original_and_redo_replays_it(
        start in "[a-z日\\n]{0,30}",
        ops in proptest::collection::vec((any::<bool>(), any::<prop::sample::Index>(), "[xy日\\n]{1,3}"), 1..12),
    ) {
        let mut b = Buffer::from_str(&start);
        for (insert, at, text) in &ops {
            b.break_group();
            let len = b.len_chars();
            if *insert {
                b.insert(at.index(len + 1), text);
            } else if len > 0 {
                let s = at.index(len);
                b.remove(s..(s + text.chars().count()).min(len));
            }
        }
        let finished = b.text();
        let mut n = 0;
        while b.undo() {
            n += 1;
            prop_assert!(n < 100);
        }
        prop_assert_eq!(b.text(), start);
        while b.redo() {}
        prop_assert_eq!(b.text(), finished);
    }

    #[test]
    fn line_col_and_pos_roundtrip(text in "[a-z日\\n]{0,60}", at in any::<prop::sample::Index>()) {
        let b = Buffer::from_str(&text);
        let pos = at.index(b.len_chars() + 1);
        let (l, c) = b.line_col(pos);
        prop_assert_eq!(b.pos_of(l, c), pos);
        prop_assert!(b.line_count() >= 1);
    }

    // ---------------------------------------------------------------- table layout

    #[test]
    fn table_lines_always_have_exactly_the_visible_width(
        cells in proptest::collection::vec(proptest::collection::vec("\\PC{0,30}", 1..5), 0..12),
        vw in 1usize..60, vh in 1usize..15, col_off in 0usize..80, row_off in 0usize..20,
    ) {
        let ncols = cells.iter().map(Vec::len).max().unwrap_or(1);
        let cols: Vec<Column> = (0..ncols).map(|i| Column { name: format!("c{i}"), type_name: "t".into() }).collect();
        let opts = TableOpts { max_width: 40, max_rows: 6, column_max_width: 12 };
        let st = TableState { row_off, col_off, ..Default::default() };
        let r = table::layout(&cols, &cells, &opts, &st, vw, vh);
        prop_assert!(r.visible_w <= vw.min(40));
        prop_assert!(r.rows.len() <= vh.min(6));
        for line in [&r.header, &r.separator].into_iter().chain(r.rows.iter()) {
            prop_assert_eq!(table::display_width(line), r.visible_w);
        }
        prop_assert!(r.row_off <= r.total_rows);
        prop_assert!(r.col_off <= r.total_w);
    }

    #[test]
    fn fit_is_always_exact_width(s in "\\PC{0,40}", w in 0usize..30) {
        prop_assert_eq!(table::display_width(&table::fit(&s, w)), w);
    }

    #[test]
    fn slice_cols_is_always_exact_width(s in "\\PC{0,40}", start in 0usize..60, w in 0usize..30) {
        prop_assert_eq!(table::display_width(&table::slice_cols(&s, start, w)), w);
    }

    // ---------------------------------------------------------------- suggestions

    #[test]
    fn suggestions_are_distinct_words_of_the_text_matching_the_prefix(text in "[a-zA-Z_ \\n]{0,80}", at in any::<prop::sample::Index>()) {
        let cursor = at.index(text.chars().count() + 1);
        if let Some(s) = suggest(&text, cursor, 2, 8) {
            prop_assert!(s.items.len() <= 8 && !s.items.is_empty());
            let mut seen = std::collections::HashSet::new();
            let words: std::collections::HashSet<&str> = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
            for w in &s.items {
                prop_assert!(seen.insert(w.clone()), "duplicate {w}");
                prop_assert!(words.contains(w.as_str()), "{w} is not in the text");
                prop_assert!(w.to_lowercase().contains(&s.prefix.to_lowercase()));
                prop_assert!(w != &s.prefix);
            }
        }
    }
}
