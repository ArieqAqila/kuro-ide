//! Word-based suggestions, like VS Code's: complete the word being typed from words already in the file.
//!
//! Ranking: words that *start with* the typed prefix come before words that merely *contain* it
//! (both ignoring case); within a group, the word whose nearest occurrence is closest to the cursor
//! comes first, then alphabetical.

use crate::buffer::is_word_char;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestions {
    /// Char index where the typed prefix starts; accepting replaces `prefix_start..cursor`.
    pub prefix_start: usize,
    pub prefix: String,
    pub items: Vec<String>,
}

/// Suggestions for the word ending at `cursor` (a char index into `text`), or `None` when the
/// cursor is not in a word, the prefix is shorter than `min_chars`, or nothing matches.
pub fn suggest(text: &str, cursor: usize, min_chars: usize, limit: usize) -> Option<Suggestions> {
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    let mut start = cursor;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let prefix: String = chars[start..cursor].iter().collect();
    if prefix.chars().count() < min_chars.max(1) || prefix.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut end = cursor;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    let current: String = chars[start..end].iter().collect();
    let low_prefix = prefix.to_lowercase();

    // word -> distance of its nearest occurrence to the cursor
    let mut nearest: HashMap<String, usize> = HashMap::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_word_char(chars[i]) {
            i += 1;
            continue;
        }
        let s = i;
        while i < chars.len() && is_word_char(chars[i]) {
            i += 1;
        }
        if s == start {
            continue; // the word being typed is not a suggestion for itself
        }
        let w: String = chars[s..i].iter().collect();
        if w == current || w == prefix || w.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let d = s.abs_diff(cursor).min(i.abs_diff(cursor));
        nearest.entry(w).and_modify(|e| *e = (*e).min(d)).or_insert(d);
    }

    let mut ranked: Vec<(u8, usize, String, String)> = nearest
        .into_iter()
        .filter_map(|(w, d)| {
            let lw = w.to_lowercase();
            let rank = if lw.starts_with(&low_prefix) {
                0
            } else if lw.contains(&low_prefix) {
                1
            } else {
                return None;
            };
            Some((rank, d, lw, w))
        })
        .collect();
    ranked.sort();
    let items: Vec<String> = ranked.into_iter().take(limit).map(|(_, _, _, w)| w).collect();
    if items.is_empty() { None } else { Some(Suggestions { prefix_start: start, prefix, items }) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Suggestions with the cursor at the `|` in `src` (min 2 chars, up to 8 items).
    fn at(src: &str) -> Option<Suggestions> {
        at_min(src, 2)
    }

    fn at_min(src: &str, min: usize) -> Option<Suggestions> {
        let pos = src.chars().position(|c| c == '|').unwrap();
        let text: String = src.chars().filter(|c| *c != '|').collect();
        suggest(&text, pos, min, 8)
    }

    fn items(src: &str) -> Vec<String> {
        at(src).map(|s| s.items).unwrap_or_default()
    }

    #[test]
    fn suggests_only_words_that_are_in_the_file() {
        let got = items("alpha beta alphabet\nal|");
        assert_eq!(got, vec!["alphabet", "alpha"], "nearest first; `beta` does not match");
        assert!(items("alpha beta\nzz|").is_empty(), "nothing in the file starts with or contains zz");
    }

    #[test]
    fn the_word_being_typed_is_not_suggested_alone() {
        assert!(at("hello\nhel|").is_some());
        assert_eq!(items("foo\nfoo|"), Vec::<String>::new(), "an identical earlier word adds nothing to complete");
    }

    #[test]
    fn the_typed_word_itself_is_never_listed() {
        // `foob` is being typed; the other words match, but not the typed occurrence
        assert_eq!(items("foobar foobaz\nfoob|"), vec!["foobaz", "foobar"]);
        assert!(items("foob|").is_empty(), "a lone word has nothing to suggest");
    }

    #[test]
    fn matching_ignores_case_and_keeps_the_original_spelling() {
        assert_eq!(items("HelloWorld\nhel|"), vec!["HelloWorld"]);
        assert_eq!(items("helloworld\nHEL|"), vec!["helloworld"]);
        assert_eq!(items("Alpha ALPHA alpha\nalp|").len(), 3, "different spellings are different words");
    }

    #[test]
    fn prefix_matches_rank_before_substring_matches() {
        let got = items("bar carry array\nar|");
        assert_eq!(got[0], "array", "the only word that starts with `ar`: {got:?}");
        assert_eq!(got.len(), 3);
        assert!(got.contains(&"bar".to_string()) && got.contains(&"carry".to_string()), "substring matches follow: {got:?}");
    }

    #[test]
    fn within_a_group_the_word_nearest_the_cursor_wins() {
        let near = items("far_word xx\nnear_word xx\nwo|");
        assert_eq!(near, vec!["near_word", "far_word"], "both contain `wo`; near_word is closer");
        let got = items("apple_far\n\n\n\n\n\n\napple_near\nap|");
        assert_eq!(got, vec!["apple_near", "apple_far"]);
        let after = items("ap| apple_after\n\n\n\n\n\napple_much_later");
        assert_eq!(after[0], "apple_after", "words after the cursor count too");
    }

    #[test]
    fn a_word_identical_to_the_typed_prefix_is_not_a_suggestion() {
        assert!(items("he hello\nhe|").contains(&"hello".to_string()));
        assert!(!items("he hello\nhe|").contains(&"he".to_string()), "completing `he` to `he` changes nothing");
        assert!(items("he\nhe|").is_empty());
    }

    #[test]
    fn results_are_deterministic() {
        let text = "cab cat car cap\nca";
        let first = suggest(text, text.chars().count(), 2, 8).unwrap();
        for _ in 0..20 {
            assert_eq!(suggest(text, text.chars().count(), 2, 8).unwrap(), first, "HashMap order must not leak into the result");
        }
        assert_eq!(first.items.first().map(String::as_str), Some("cap"), "closest first");
    }

    #[test]
    fn accepting_replaces_the_prefix_range() {
        let s = at("foobar\nfoo|").unwrap();
        assert_eq!(s.prefix, "foo");
        assert_eq!(s.prefix_start, 7);
        let s = at("  alpha alphabet\n  al|").unwrap();
        assert_eq!((s.prefix.as_str(), s.prefix_start), ("al", 19));
    }

    #[test]
    fn min_chars_and_non_words() {
        assert!(at_min("alpha\na|", 2).is_none(), "one char is below the threshold");
        assert!(at_min("alpha\na|", 1).is_some());
        assert!(at("alpha\n|").is_none(), "cursor not in a word");
        assert!(at("alpha\nal |").is_none(), "cursor after a space: the word is finished");
        assert!(at("alpha (|").is_none());
        assert!(at("123 1234\n12|").is_none(), "numbers are not completed");
    }

    #[test]
    fn works_in_the_middle_of_a_word() {
        let s = at("foobar\nfo|obaz").unwrap();
        assert_eq!(s.prefix, "fo");
        assert_eq!(s.items, vec!["foobar"], "the word under the cursor is excluded");
    }

    #[test]
    fn unicode_words_and_underscores() {
        assert_eq!(items("naïve_var café\nna|"), vec!["naïve_var"]);
        assert_eq!(items("日本語テキスト x\n日本|"), vec!["日本語テキスト"]);
        assert_eq!(items("my_long_name\nmy_|"), vec!["my_long_name"]);
    }

    #[test]
    fn duplicates_are_collapsed_and_the_list_is_limited() {
        assert_eq!(items("same same same\nsa|"), vec!["same"]);
        let text: String = (0..20).map(|i| format!("word{i} ")).collect::<String>() + "\nwo";
        let s = suggest(&text, text.chars().count(), 2, 8).unwrap();
        assert_eq!(s.items.len(), 8);
    }
}
