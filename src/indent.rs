//! Indentation rules (pure functions, VS Code behaviour).
//!
//! Defaults: hard tabs, tab width 8. `style`, `size` and `tab_width` come from `[editor]`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Tabs,
    Spaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indent {
    pub style: Style,
    /// Columns per indent level when indenting with spaces (and per outdent step).
    pub size: usize,
    /// Display width of a tab character.
    pub tab_width: usize,
}

impl Indent {
    pub fn from_config(e: &crate::config::EditorConfig) -> Indent {
        Indent {
            style: if e.indent_style == "spaces" { Style::Spaces } else { Style::Tabs },
            size: e.indent_size.max(1),
            tab_width: e.tab_width.max(1),
        }
    }

    /// Text the Tab key inserts at display column `col`.
    pub fn tab_text(&self, col: usize) -> String {
        match self.style {
            Style::Tabs => "\t".into(),
            Style::Spaces => " ".repeat(self.size - col % self.size),
        }
    }

    /// Text added to the start of a line when indenting it.
    pub fn level(&self) -> String {
        match self.style {
            Style::Tabs => "\t".into(),
            Style::Spaces => " ".repeat(self.size),
        }
    }
}

/// Display width of `s` starting at display column 0, expanding tabs.
pub fn display_cols(s: &str, tab_width: usize) -> usize {
    let mut x = 0;
    for c in s.chars() {
        x += if c == '\t' { tab_width - x % tab_width } else { unicode_width::UnicodeWidthChar::width(c).unwrap_or(0) };
    }
    x
}

fn is_ws(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Number of leading whitespace chars of `line`.
pub fn leading_ws(line: &str) -> usize {
    line.chars().take_while(|c| is_ws(*c)).count()
}

/// Chars to remove from the start of `line` for one outdent (Shift+Tab): one tab, or leading
/// whitespace up to `size` columns (spaces; a tab counts up to its next stop).
pub fn outdent_len(ind: &Indent, line: &str) -> usize {
    let mut width = 0;
    let mut n = 0;
    for c in line.chars() {
        if width >= ind.size {
            break;
        }
        match c {
            ' ' => width += 1,
            '\t' => width = (width / ind.tab_width + 1) * ind.tab_width,
            _ => break,
        }
        n += 1;
    }
    n
}

/// Backspace: how many chars before the cursor to delete. `before` is the line text before the cursor.
///
/// * in leading indentation: one indent step (a tab is one step; spaces go back to the previous
///   indent stop), never a single space out of a tab-sized indent
/// * after text, on a whitespace run (when `smart`): the whole run
/// * otherwise: one char
pub fn backspace_len(ind: &Indent, before: &str, smart: bool) -> usize {
    let Some(last) = before.chars().next_back() else { return 0 };
    let all_ws = before.chars().all(is_ws);
    if all_ws {
        if last == '\t' {
            return 1;
        }
        let col = display_cols(before, ind.tab_width);
        let stop = (col - 1) / ind.size * ind.size;
        let spaces = before.chars().rev().take_while(|c| *c == ' ').count();
        return (col - stop).min(spaces).max(1);
    }
    if smart && is_ws(last) {
        return before.chars().rev().take_while(|c| is_ws(*c)).count();
    }
    1
}

/// Delete: how many chars after the cursor to delete. `after` is the line text after the cursor
/// (empty at the end of a line: the caller then deletes the line break).
pub fn delete_len(after: &str, smart: bool) -> usize {
    match after.chars().next() {
        None => 0,
        Some(c) if smart && is_ws(c) => after.chars().take_while(|c| is_ws(*c)).count(),
        Some(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs8() -> Indent {
        Indent { style: Style::Tabs, size: 8, tab_width: 8 }
    }
    fn spaces(n: usize) -> Indent {
        Indent { style: Style::Spaces, size: n, tab_width: 8 }
    }

    #[test]
    fn tab_text_follows_style_and_stops() {
        assert_eq!(tabs8().tab_text(3), "\t");
        assert_eq!(spaces(4).tab_text(0), "    ");
        assert_eq!(spaces(4).tab_text(1), "   ");
        assert_eq!(spaces(4).tab_text(4), "    ");
        assert_eq!(spaces(2).tab_text(3), " ");
        assert_eq!(tabs8().level(), "\t");
        assert_eq!(spaces(2).level(), "  ");
    }

    #[test]
    fn display_cols_expands_tabs_and_wide_chars() {
        assert_eq!(display_cols("\t", 8), 8);
        assert_eq!(display_cols("ab\t", 8), 8);
        assert_eq!(display_cols("\t\t", 4), 8);
        assert_eq!(display_cols("日本", 8), 4);
        assert_eq!(display_cols("", 8), 0);
    }

    #[test]
    fn outdent_removes_one_tab_or_up_to_size_spaces() {
        assert_eq!(outdent_len(&tabs8(), "\tfoo"), 1);
        assert_eq!(outdent_len(&tabs8(), "\t\tfoo"), 1, "one level at a time");
        assert_eq!(outdent_len(&spaces(4), "      foo"), 4);
        assert_eq!(outdent_len(&spaces(4), "  foo"), 2, "fewer spaces than a level: remove them all");
        assert_eq!(outdent_len(&spaces(4), "foo"), 0);
        assert_eq!(outdent_len(&spaces(4), ""), 0);
        assert_eq!(outdent_len(&spaces(4), "\tfoo"), 1, "a tab is one level even in spaces mode");
        assert_eq!(outdent_len(&spaces(8), "  \tfoo"), 3, "mixed: spaces then the tab that reaches the stop");
        assert_eq!(outdent_len(&spaces(2), "   x"), 2);
    }

    #[test]
    fn backspace_in_leading_indentation_removes_one_indent_not_one_space() {
        assert_eq!(backspace_len(&tabs8(), "\t", true), 1);
        assert_eq!(backspace_len(&tabs8(), "\t\t", true), 1);
        assert_eq!(backspace_len(&spaces(4), "        ", true), 4, "8 spaces, size 4: back to the previous stop");
        assert_eq!(backspace_len(&spaces(4), "      ", true), 2, "6 spaces: back to column 4");
        assert_eq!(backspace_len(&spaces(4), " ", true), 1);
        assert_eq!(backspace_len(&spaces(8), "   ", true), 3);
        assert_eq!(backspace_len(&spaces(4), "\t    ", true), 4, "tab then spaces: only the spaces step");
        assert_eq!(backspace_len(&spaces(4), "    \t", true), 1, "a tab before the cursor is one char");
    }

    #[test]
    fn backspace_after_text_removes_the_whole_whitespace_run() {
        assert_eq!(backspace_len(&tabs8(), "foo   ", true), 3);
        assert_eq!(backspace_len(&tabs8(), "foo \t ", true), 3, "mixed run");
        assert_eq!(backspace_len(&tabs8(), "foo ", true), 1);
        assert_eq!(backspace_len(&tabs8(), "foo", true), 1);
        assert_eq!(backspace_len(&tabs8(), "foo   ", false), 1, "smart whitespace delete can be turned off");
    }

    #[test]
    fn backspace_edge_cases() {
        assert_eq!(backspace_len(&tabs8(), "", true), 0, "start of line: caller joins lines");
        assert_eq!(backspace_len(&tabs8(), "日本", true), 1);
        assert_eq!(backspace_len(&spaces(4), "x\t", true), 1);
    }

    #[test]
    fn delete_removes_the_whole_whitespace_run() {
        assert_eq!(delete_len("   foo", true), 3);
        assert_eq!(delete_len("\t\tfoo", true), 2);
        assert_eq!(delete_len(" \t foo", true), 3);
        assert_eq!(delete_len("foo", true), 1);
        assert_eq!(delete_len("   ", true), 3, "whitespace to the end of the line");
        assert_eq!(delete_len("", true), 0, "end of line: caller deletes the line break");
        assert_eq!(delete_len("   foo", false), 1);
    }

    #[test]
    fn leading_ws_counts_chars() {
        assert_eq!(leading_ws("\t  x"), 3);
        assert_eq!(leading_ws(""), 0);
        assert_eq!(leading_ws("   "), 3);
    }
}
