//! Pure find / replace engine. Ranges are in **chars** (not bytes) so they can
//! be used directly against the rope buffer.

use regex::RegexBuilder;
use std::ops::Range;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Finder {
    pub query: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FindError {
    InvalidRegex(String),
}

impl std::fmt::Display for FindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FindError::InvalidRegex(m) => write!(f, "invalid regex: {m}"),
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A match is "whole word" when each edge that is itself a word character is
/// not adjacent to another word character (so `-x` and `(foo` still work).
fn word_bounded(text: &str, start: usize, end: usize) -> bool {
    let m = &text[start..end];
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    let first = m.chars().next();
    let last = m.chars().next_back();
    let left_ok = !(first.is_some_and(is_word) && before.is_some_and(is_word));
    let right_ok = !(last.is_some_and(is_word) && after.is_some_and(is_word));
    left_ok && right_ok
}

impl Finder {
    pub fn new(query: &str) -> Self {
        Finder { query: query.to_string(), ..Default::default() }
    }

    fn scan(&self, text: &str, repl: Option<&str>) -> Result<Vec<(Range<usize>, String)>, FindError> {
        if self.query.is_empty() {
            return Ok(vec![]);
        }
        let pat = if self.regex { self.query.clone() } else { regex::escape(&self.query) };
        let re = RegexBuilder::new(&pat)
            .case_insensitive(!self.case_sensitive)
            .multi_line(true)
            .build()
            .map_err(|e| FindError::InvalidRegex(e.to_string().lines().last().unwrap_or("").trim().to_string()))?;
        let mut out = Vec::new();
        let (mut char_pos, mut byte_pos) = (0usize, 0usize);
        for caps in re.captures_iter(text) {
            let m = caps.get(0).unwrap();
            if m.start() == m.end() {
                continue;
            }
            if self.whole_word && !word_bounded(text, m.start(), m.end()) {
                continue;
            }
            char_pos += text[byte_pos..m.start()].chars().count();
            let len = m.as_str().chars().count();
            let rep = match repl {
                None => String::new(),
                Some(r) if self.regex => {
                    let mut d = String::new();
                    caps.expand(r, &mut d);
                    d
                }
                Some(r) => r.to_string(),
            };
            out.push((char_pos..char_pos + len, rep));
            char_pos += len;
            byte_pos = m.end();
        }
        Ok(out)
    }

    /// All non-empty matches, as char ranges, in document order.
    pub fn find_all(&self, text: &str) -> Result<Vec<Range<usize>>, FindError> {
        Ok(self.scan(text, None)?.into_iter().map(|(r, _)| r).collect())
    }

    pub fn count(&self, text: &str) -> Result<usize, FindError> {
        Ok(self.find_all(text)?.len())
    }

    /// Every match paired with its replacement text (`$1` expands in regex mode).
    pub fn replacements(&self, text: &str, repl: &str) -> Result<Vec<(Range<usize>, String)>, FindError> {
        self.scan(text, Some(repl))
    }

    pub fn replace_all(&self, text: &str, repl: &str) -> Result<(String, usize), FindError> {
        let edits = self.replacements(text, repl)?;
        let n = edits.len();
        Ok((apply_replacements(text, &edits), n))
    }

    /// Replace only the `index`-th match.
    pub fn replace_nth(&self, text: &str, repl: &str, index: usize) -> Result<Option<String>, FindError> {
        let edits = self.replacements(text, repl)?;
        Ok(edits.get(index).map(|e| apply_replacements(text, std::slice::from_ref(e))))
    }
}

/// Apply non-overlapping, ascending char-range edits.
pub fn apply_replacements(text: &str, edits: &[(Range<usize>, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    let mut pos = 0;
    for (r, s) in edits {
        out.extend(chars.by_ref().take(r.start - pos));
        chars.by_ref().take(r.end - r.start).for_each(drop);
        out.push_str(s);
        pos = r.end;
    }
    out.extend(chars);
    out
}

/// Index of the first match starting strictly after `pos`, wrapping to the first.
pub fn next_match(matches: &[Range<usize>], pos: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.iter().position(|m| m.start > pos).unwrap_or(0))
}

/// Index of the last match starting strictly before `pos`, wrapping to the last.
pub fn prev_match(matches: &[Range<usize>], pos: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.iter().rposition(|m| m.start < pos).unwrap_or(matches.len() - 1))
}

/// First match starting at or after `pos` (wrapping): used for incremental search.
pub fn match_at_or_after(matches: &[Range<usize>], pos: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.iter().position(|m| m.start >= pos).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(q: &str) -> Finder {
        Finder::new(q)
    }

    #[test]
    fn empty_query_matches_nothing() {
        assert_eq!(f("").count("abc").unwrap(), 0);
    }

    #[test]
    fn case_insensitive_by_default() {
        assert_eq!(f("foo").find_all("Foo foo FOO").unwrap(), vec![0..3, 4..7, 8..11]);
    }

    #[test]
    fn case_sensitive() {
        let mut x = f("foo");
        x.case_sensitive = true;
        assert_eq!(x.find_all("Foo foo FOO").unwrap(), vec![4..7]);
    }

    #[test]
    fn whole_word() {
        let mut x = f("foo");
        x.whole_word = true;
        assert_eq!(x.count("foo foobar barfoo foo_ (foo) foo").unwrap(), 3);
        assert_eq!(x.find_all("foobar foo").unwrap(), vec![7..10]);
    }

    #[test]
    fn whole_word_with_non_word_edge() {
        let mut x = f("-x");
        x.whole_word = true;
        assert_eq!(x.count("a-x -x").unwrap(), 2);
    }

    #[test]
    fn literal_mode_escapes_regex_chars() {
        assert_eq!(f("a.c").count("abc a.c").unwrap(), 1);
        assert_eq!(f("(").count("f(x)").unwrap(), 1);
    }

    #[test]
    fn regex_mode() {
        let mut x = f(r"\d+");
        x.regex = true;
        assert_eq!(x.find_all("a1 b22 c333").unwrap(), vec![1..2, 4..6, 8..11]);
    }

    #[test]
    fn invalid_regex_is_an_error_not_a_panic() {
        let mut x = f("(");
        x.regex = true;
        assert!(matches!(x.find_all("abc"), Err(FindError::InvalidRegex(_))));
    }

    #[test]
    fn empty_regex_matches_are_skipped() {
        let mut x = f("x*");
        x.regex = true;
        assert_eq!(x.find_all("abxxc").unwrap(), vec![2..4]);
    }

    #[test]
    fn ranges_are_in_chars_not_bytes() {
        assert_eq!(f("b").find_all("é€b😀b").unwrap(), vec![2..3, 4..5]);
    }

    #[test]
    fn multiline_anchors() {
        let mut x = f("^b");
        x.regex = true;
        assert_eq!(x.find_all("a\nb\nb").unwrap(), vec![2..3, 4..5]);
    }

    #[test]
    fn next_wraps_around() {
        let m = vec![0..1, 5..6, 9..10];
        assert_eq!(next_match(&m, 0), Some(1));
        assert_eq!(next_match(&m, 5), Some(2));
        assert_eq!(next_match(&m, 9), Some(0), "wraps to first");
        assert_eq!(next_match(&[], 3), None);
    }

    #[test]
    fn prev_wraps_around() {
        let m = vec![0..1, 5..6, 9..10];
        assert_eq!(prev_match(&m, 9), Some(1));
        assert_eq!(prev_match(&m, 5), Some(0));
        assert_eq!(prev_match(&m, 0), Some(2), "wraps to last");
        assert_eq!(prev_match(&[], 3), None);
    }

    #[test]
    fn incremental_match_selection() {
        let m = vec![2..3, 7..8];
        assert_eq!(match_at_or_after(&m, 2), Some(0));
        assert_eq!(match_at_or_after(&m, 3), Some(1));
        assert_eq!(match_at_or_after(&m, 8), Some(0));
    }

    #[test]
    fn replace_all_literal() {
        let (s, n) = f("cat").replace_all("cat Cat dog cat", "bird").unwrap();
        assert_eq!((s.as_str(), n), ("bird bird dog bird", 3));
    }

    #[test]
    fn replace_all_regex_with_captures() {
        let mut x = f(r"(\w+)@(\w+)");
        x.regex = true;
        let (s, n) = x.replace_all("a@b c@d", "$2:$1").unwrap();
        assert_eq!((s.as_str(), n), ("b:a d:c", 2));
    }

    #[test]
    fn literal_replacement_does_not_expand_dollars() {
        let (s, _) = f("a").replace_all("a", "$1").unwrap();
        assert_eq!(s, "$1");
    }

    #[test]
    fn replace_nth_only_touches_that_match() {
        assert_eq!(f("a").replace_nth("a a a", "X", 1).unwrap().unwrap(), "a X a");
        assert_eq!(f("a").replace_nth("a", "X", 5).unwrap(), None);
    }

    #[test]
    fn replace_with_unicode_and_length_change() {
        let (s, n) = f("é").replace_all("éaé", "ee").unwrap();
        assert_eq!((s.as_str(), n), ("eeaee", 2));
    }

    #[test]
    fn replace_no_match_returns_input() {
        let (s, n) = f("zzz").replace_all("abc", "x").unwrap();
        assert_eq!((s.as_str(), n), ("abc", 0));
    }
}
