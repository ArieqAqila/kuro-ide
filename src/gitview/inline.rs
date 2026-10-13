//! Where inside a changed line the change is: for every `-` line that is followed by a `+` line (and the other way round) the words that
//! differ are marked, so the eye goes to `20260710` → `20260721` and not only to "this line is different".
//!
//! Ranges are in terminal cells of the whole diff line (the leading `+` / `-` is cell 0 and is never marked), so they stay right when the
//! diff is scrolled sideways. They are drawn in reverse video, never with a background colour.

use crate::table::display_width;
use unicode_segmentation::UnicodeSegmentation;

/// Lines longer than this many tokens are compared by their common start and end only (the exact comparison grows with the square).
const MAX_TOKENS: usize = 400;

/// Cell ranges `(start, end)` within a line.
pub type Ranges = Vec<(usize, usize)>;

#[derive(Clone)]
struct Tok {
    text: String,
    start: usize,
    end: usize,
}

#[derive(PartialEq, Clone, Copy)]
enum Class {
    Word,
    Space,
    Other,
}

fn class(g: &str) -> Class {
    let c = g.chars().next().unwrap_or(' ');
    if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Other
    }
}

/// Words, runs of white space, and single punctuation marks, with the cells they occupy (the first cell, the `+`/`-`, is skipped).
fn tokens(line: &str) -> Vec<Tok> {
    let mut out: Vec<Tok> = vec![];
    let mut x = 0;
    let mut prev: Option<Class> = None;
    for (i, g) in line.graphemes(true).enumerate() {
        let w = display_width(g);
        if i > 0 {
            let c = class(g);
            let joins = matches!(c, Class::Word | Class::Space) && prev == Some(c);
            if joins && let Some(t) = out.last_mut() {
                t.text.push_str(g);
                t.end = x + w;
            } else {
                out.push(Tok { text: g.to_string(), start: x, end: x + w });
            }
            prev = Some(c);
        }
        x += w;
    }
    out
}

/// Pairs of equal tokens `(index in a, index in b)` that form a longest common subsequence.
fn matches(a: &[Tok], b: &[Tok]) -> Vec<(usize, usize)> {
    let (mut lo, mut hi_a, mut hi_b) = (0, a.len(), b.len());
    let mut head = vec![];
    while lo < hi_a && lo < hi_b && a[lo].text == b[lo].text {
        head.push((lo, lo));
        lo += 1;
    }
    let mut tail = vec![];
    while hi_a > lo && hi_b > lo && a[hi_a - 1].text == b[hi_b - 1].text {
        hi_a -= 1;
        hi_b -= 1;
        tail.push((hi_a, hi_b));
    }
    tail.reverse();
    let (ma, mb) = (&a[lo..hi_a], &b[lo..hi_b]);
    let mut mid = vec![];
    if !ma.is_empty() && !mb.is_empty() && ma.len() <= MAX_TOKENS && mb.len() <= MAX_TOKENS {
        let (n, m) = (ma.len(), mb.len());
        let mut dp = vec![vec![0u16; m + 1]; n + 1];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                dp[i][j] = if ma[i].text == mb[j].text { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if ma[i].text == mb[j].text {
                mid.push((lo + i, lo + j));
                i += 1;
                j += 1;
            } else if dp[i + 1][j] >= dp[i][j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    head.into_iter().chain(mid).chain(tail).collect()
}

/// The graphemes of tokens `from..to` with their cells.
fn graphemes(toks: &[Tok]) -> Vec<(String, usize, usize)> {
    let mut out = vec![];
    for t in toks {
        let mut x = t.start;
        for g in t.text.graphemes(true) {
            let w = display_width(g);
            out.push((g.to_string(), x, x + w));
            x += w;
        }
    }
    out
}

/// What differs between two lines: the cell ranges to mark in `a` and in `b`. Nothing when they share no word at all (the whole
/// line is different anyway, marking all of it would only be noise).
pub fn pair(a: &str, b: &str) -> (Ranges, Ranges) {
    let (ta, tb) = (tokens(a), tokens(b));
    let m = matches(&ta, &tb);
    let shared_word = m.iter().any(|&(i, _)| class(&ta[i].text) == Class::Word);
    if !shared_word {
        return (vec![], vec![]);
    }
    let (mut ra, mut rb) = (vec![], vec![]);
    // the stretches between two matches, and before the first and after the last, are what changed; inside each, what the two sides
    // still have in common at the start and at the end (the `10` of `20260710` against `21` of `20260721`) is not marked
    let mut anchors: Vec<(i64, i64)> = vec![(-1, -1)];
    anchors.extend(m.iter().map(|&(i, j)| (i as i64, j as i64)));
    anchors.push((ta.len() as i64, tb.len() as i64));
    for w in anchors.windows(2) {
        let ga = graphemes(&ta[(w[0].0 + 1) as usize..w[1].0 as usize]);
        let gb = graphemes(&tb[(w[0].1 + 1) as usize..w[1].1 as usize]);
        let mut p = 0;
        while p < ga.len() && p < gb.len() && ga[p].0 == gb[p].0 {
            p += 1;
        }
        let mut s = 0;
        while s < ga.len() - p && s < gb.len() - p && ga[ga.len() - 1 - s].0 == gb[gb.len() - 1 - s].0 {
            s += 1;
        }
        if p < ga.len() - s {
            ra.push((ga[p].1, ga[ga.len() - s - 1].2));
        }
        if p < gb.len() - s {
            rb.push((gb[p].1, gb[gb.len() - s - 1].2));
        }
    }
    (merge(ra), merge(rb))
}

/// Ranges that touch or are only a space apart become one block.
fn merge(mut r: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    r.sort_unstable();
    let mut out: Vec<(usize, usize)> = vec![];
    for (s, e) in r {
        match out.last_mut() {
            Some(l) if s <= l.1 => l.1 = l.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// The cell ranges to mark for every line of a diff (an empty list for lines with nothing to mark).
pub fn marks(lines: &[String]) -> Vec<Vec<(usize, usize)>> {
    let mut out: Vec<Vec<(usize, usize)>> = vec![vec![]; lines.len()];
    let mut in_hunk = false;
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        if l.starts_with("diff --git") || l.starts_with("diff --cc") {
            in_hunk = false;
        } else if l.starts_with("@@") {
            in_hunk = true;
        }
        if !in_hunk || !(l.starts_with('-') || l.starts_with('+')) {
            i += 1;
            continue;
        }
        // a block: removed lines, then added lines
        let dels: Vec<usize> = (i..lines.len()).take_while(|&k| lines[k].starts_with('-')).collect();
        let after = i + dels.len();
        let adds: Vec<usize> = (after..lines.len()).take_while(|&k| lines[k].starts_with('+')).collect();
        for (d, a) in dels.iter().zip(&adds) {
            let (rd, ra) = pair(&lines[*d], &lines[*a]);
            out[*d] = rd;
            out[*a] = ra;
        }
        i = after + adds.len();
        if dels.is_empty() && adds.is_empty() {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(line: &str, r: &[(usize, usize)]) -> Vec<String> {
        // the text under each range, by cells (all ASCII in these tests)
        r.iter().map(|&(s, e)| line[s..e].to_string()).collect()
    }

    #[test]
    fn only_the_digits_that_changed_are_marked() {
        let (a, b) = ("-lversi        INIT        \"20260710\"", "+lversi        INIT        \"20260721\"");
        let (ra, rb) = pair(a, b);
        assert_eq!(shown(a, &ra), vec!["10"]);
        assert_eq!(shown(b, &rb), vec!["21"]);
    }

    #[test]
    fn a_replaced_word_is_marked_on_both_sides_and_the_rest_is_not() {
        let (a, b) = ("-sgmdatstrbrg    lsgmdata tbtcab lsgmstr1 lsgmstr2", "+sgmdatstrbrg    lsgmdata lM2W lsgmstr1 lsgmstr2");
        let (ra, rb) = pair(a, b);
        assert_eq!(shown(a, &ra), vec!["tbtcab"]);
        assert_eq!(shown(b, &rb), vec!["lM2W"]);
    }

    #[test]
    fn a_word_that_shares_its_start_marks_only_the_rest() {
        let (a, b) = ("-tbtket    ldsbkode2    REPLACE", "+tbttblkode    ldsbkode2    REPLACE");
        let (ra, rb) = pair(a, b);
        assert_eq!(shown(a, &ra), vec!["ket"], "tbt|ket vs tbt|tblkode: the common start stays unmarked");
        assert_eq!(shown(b, &rb), vec!["tblkode"]);
    }

    #[test]
    fn lines_with_no_word_in_common_get_no_marks() {
        assert_eq!(pair("-alpha beta", "+gamma delta"), (vec![], vec![]));
    }

    #[test]
    fn identical_text_has_nothing_to_mark() {
        assert_eq!(pair("-same line", "+same line"), (vec![], vec![]));
    }

    #[test]
    fn the_plus_and_minus_are_never_marked_and_added_text_at_the_end_is() {
        let (a, b) = ("-call(x)", "+call(x, y)");
        let (ra, rb) = pair(a, b);
        assert!(ra.is_empty(), "nothing was removed: {ra:?}");
        assert!(rb.iter().all(|r| r.0 >= 1));
        assert_eq!(shown(b, &rb).concat(), ", y");
    }

    #[test]
    fn wide_characters_and_tabs_as_spaces_are_counted_in_cells() {
        let (a, b) = ("-名前 = 1", "+名前 = 2");
        let (ra, rb) = pair(a, b);
        // `-` is cell 0, 名前 takes cells 1..5, then ` = ` and the digit at cell 8
        assert_eq!((ra, rb), (vec![(8, 9)], vec![(8, 9)]));
    }

    #[test]
    fn a_block_pairs_the_first_removed_line_with_the_first_added_one() {
        let lines: Vec<String> =
            ["diff --git a/f b/f", "--- a/f", "+++ b/f", "@@ -1,3 +1,3 @@", " keep", "-a 1", "-b 2", "+a 9", "+b 2", " keep"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let m = marks(&lines);
        assert!(m[1].is_empty() && m[2].is_empty(), "file headers are not content");
        assert_eq!(m[5], vec![(3, 4)]);
        assert_eq!(m[7], vec![(3, 4)]);
        assert!(m[6].is_empty() && m[8].is_empty(), "b 2 is unchanged");
        assert!(m[4].is_empty() && m[9].is_empty());
    }

    #[test]
    fn lines_added_or_removed_alone_are_not_marked() {
        let lines: Vec<String> = ["@@ -1 +1,2 @@", " x", "+new line", " y"].iter().map(|s| s.to_string()).collect();
        assert!(marks(&lines).iter().all(|m| m.is_empty()));
    }

    #[test]
    fn very_long_lines_do_not_blow_up() {
        let a = format!("-{}", "x ".repeat(5000));
        let b = format!("+{}y", "x ".repeat(5000));
        let (_, rb) = pair(&a, &b);
        assert!(!rb.is_empty());
    }
}
