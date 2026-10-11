//! Find / replace bar: state, geometry, and drawing. The bar floats in the
//! top-right corner of the text area, like VS Code's.

use crate::buffer::Buffer;
use crate::config::FindConfig;
use crate::find::{FindError, Finder, match_at_or_after};
use ratatui::buffer::Buffer as Cells;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, BorderType, Clear, Widget};
use std::ops::Range;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Default)]
pub struct TextField {
    pub text: String,
    /// Cursor position in chars.
    pub cursor: usize,
}

impl TextField {
    fn byte(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map(|(i, _)| i).unwrap_or(self.text.len())
    }
    pub fn len(&self) -> usize {
        self.text.chars().count()
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    pub fn insert_str(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        let b = self.byte(self.cursor);
        self.text.insert_str(b, &s);
        self.cursor += s.chars().count();
    }
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let (a, b) = (self.byte(self.cursor - 1), self.byte(self.cursor));
            self.text.replace_range(a..b, "");
            self.cursor -= 1;
        }
    }
    pub fn delete(&mut self) {
        if self.cursor < self.len() {
            let (a, b) = (self.byte(self.cursor), self.byte(self.cursor + 1));
            self.text.replace_range(a..b, "");
        }
    }
    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }
    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }
    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self) {
        self.cursor = self.len();
    }
    pub fn kill_to_start(&mut self) {
        let b = self.byte(self.cursor);
        self.text.replace_range(..b, "");
        self.cursor = 0;
    }
    pub fn set(&mut self, s: &str) {
        self.text = s.to_string();
        self.cursor = self.len();
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
}

#[derive(Debug, Clone)]
pub struct FindBar {
    pub open: bool,
    pub replace_mode: bool,
    pub in_replace: bool,
    pub query: TextField,
    pub repl: TextField,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub matches: Vec<Range<usize>>,
    pub error: Option<String>,
    computed_for: Option<(Finder, u64)>,
}

impl FindBar {
    pub fn new(cfg: &FindConfig) -> Self {
        FindBar {
            open: false,
            replace_mode: false,
            in_replace: false,
            query: TextField::default(),
            repl: TextField::default(),
            case_sensitive: cfg.case_sensitive,
            whole_word: cfg.whole_word,
            regex: cfg.regex,
            matches: vec![],
            error: None,
            computed_for: None,
        }
    }

    pub fn finder(&self) -> Finder {
        Finder { query: self.query.text.clone(), case_sensitive: self.case_sensitive, whole_word: self.whole_word, regex: self.regex }
    }

    /// Recompute matches if the query, options or buffer changed.
    pub fn refresh(&mut self, buf: &Buffer) {
        if !self.open {
            return;
        }
        let f = self.finder();
        if self.computed_for.as_ref().is_some_and(|(pf, v)| *pf == f && *v == buf.version) {
            return;
        }
        match f.find_all(&buf.text()) {
            Ok(m) => {
                self.matches = m;
                self.error = None;
            }
            Err(FindError::InvalidRegex(e)) => {
                self.matches.clear();
                self.error = Some(e);
            }
        }
        self.computed_for = Some((f, buf.version));
    }

    /// Index of the match equal to the current selection, if any.
    pub fn current_index(&self, buf: &Buffer) -> Option<usize> {
        let sel = buf.selection()?;
        self.matches.iter().position(|m| *m == sel)
    }

    pub fn count_label(&self, buf: &Buffer) -> String {
        if self.error.is_some() {
            return "Bad regex".into();
        }
        if self.query.is_empty() {
            return String::new();
        }
        if self.matches.is_empty() {
            return "No results".into();
        }
        match self.current_index(buf) {
            Some(i) => format!("{} of {}", i + 1, self.matches.len()),
            None => format!("? of {}", self.matches.len()),
        }
    }

    /// The match to select when the query changes (nearest at/after the cursor).
    pub fn incremental_target(&self, cursor: usize) -> Option<usize> {
        match_at_or_after(&self.matches, cursor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarLayout {
    pub rect: Rect,
    pub input: Rect,
    pub count: Rect,
    pub case_btn: Rect,
    pub word_btn: Rect,
    pub regex_btn: Rect,
    pub prev_btn: Rect,
    pub next_btn: Rect,
    pub close_btn: Rect,
    pub repl_input: Option<Rect>,
    pub repl_one: Option<Rect>,
    pub repl_all: Option<Rect>,
}

pub const BAR_WIDTH: u16 = 48;

/// Geometry of the bar inside `area` (the text area).
pub fn bar_layout(area: Rect, replace: bool) -> BarLayout {
    let w = BAR_WIDTH.min(area.width.saturating_sub(1)).max(area.width.min(36));
    let h: u16 = if replace { 4 } else { 3 }.min(area.height.max(1));
    let x = area.right().saturating_sub(w + 1).max(area.x);
    let rect = Rect::new(x, area.y, w.min(area.width), h);
    let inner = Rect::new(rect.x + 1, rect.y + 1, rect.width.saturating_sub(2), h.saturating_sub(2));
    let r = inner.right();
    let at = |dx: u16, w: u16, row: u16| Rect::new(r.saturating_sub(dx), inner.y + row, w, 1);
    let input_x = inner.x + 1;
    let input_w = r.saturating_sub(28).saturating_sub(input_x).max(4);
    BarLayout {
        rect,
        input: Rect::new(input_x, inner.y, input_w, 1),
        count: at(27, 11, 0),
        case_btn: at(15, 2, 0),
        word_btn: at(12, 2, 0),
        regex_btn: at(9, 2, 0),
        prev_btn: at(6, 1, 0),
        next_btn: at(4, 1, 0),
        close_btn: at(2, 1, 0),
        repl_input: replace.then(|| Rect::new(input_x, inner.y + 1, input_w + 11, 1)),
        repl_one: replace.then(|| at(6, 1, 1)),
        repl_all: replace.then(|| at(4, 1, 1)),
    }
}

/// Whether the bar fits in `area`; tiny windows skip drawing it rather than overflow.
pub fn bar_fits(area: Rect, replace: bool) -> bool {
    area.width >= 36 && area.height >= if replace { 4 } else { 3 }
}

pub fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}

fn put(buf: &mut Cells, r: Rect, s: &str, style: Style) {
    buf.set_stringn(r.x, r.y, s, r.width as usize, style);
}

/// Draw a text field; returns the screen cursor position.
pub fn draw_field(buf: &mut Cells, r: Rect, f: &TextField, placeholder: &str, mask: bool) -> (u16, u16) {
    let chars: Vec<char> = if mask { f.text.chars().map(|_| '•').collect() } else { f.text.chars().collect() };
    let width_of = |cs: &[char]| cs.iter().map(|c| UnicodeWidthChar::width(*c).unwrap_or(0)).sum::<usize>();
    // scroll so the cursor stays visible
    let mut start = 0;
    while width_of(&chars[start..f.cursor.min(chars.len())]) >= r.width as usize && start < f.cursor {
        start += 1;
    }
    if chars.is_empty() {
        put(buf, r, placeholder, Style::default().add_modifier(Modifier::DIM));
    } else {
        let s: String = chars[start..].iter().collect();
        put(buf, r, &s, Style::default());
    }
    let cx = r.x + width_of(&chars[start..f.cursor.min(chars.len())]) as u16;
    (cx.min(r.right().saturating_sub(1)), r.y)
}

/// Draw the whole bar. Returns the screen cursor position of the focused field.
pub fn render(buf: &mut Cells, area: Rect, bar: &FindBar, b: &Buffer, in_focus: bool) -> Option<(u16, u16)> {
    if !bar_fits(area, bar.replace_mode) {
        return None;
    }
    let l = bar_layout(area, bar.replace_mode);
    Clear.render(l.rect, buf);
    Block::bordered().border_type(BorderType::Rounded).style(Style::default()).render(l.rect, buf);

    let on = |active: bool| if active { Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD) } else { Style::default() };
    let dim = Style::default().add_modifier(Modifier::DIM);

    let qfocus = in_focus && !bar.in_replace;
    let cur = draw_field(buf, l.input, &bar.query, "Find", false);
    let label = bar.count_label(b);
    let label_style = if bar.error.is_some() || label == "No results" { Style::default().add_modifier(Modifier::BOLD) } else { dim };
    put(buf, l.count, &label, label_style);
    put(buf, l.case_btn, "Aa", on(bar.case_sensitive));
    put(buf, l.word_btn, "ab", on(bar.whole_word));
    put(buf, l.regex_btn, ".*", on(bar.regex));
    put(buf, l.prev_btn, "↑", Style::default());
    put(buf, l.next_btn, "↓", Style::default());
    put(buf, l.close_btn, "✕", Style::default());
    let mut cursor = qfocus.then_some(cur);

    if let (Some(ri), Some(one), Some(all)) = (l.repl_input, l.repl_one, l.repl_all) {
        let rf = in_focus && bar.in_replace;
        let c = draw_field(buf, ri, &bar.repl, "Replace", false);
        if rf {
            cursor = Some(c);
        }
        put(buf, one, "↵", Style::default());
        put(buf, all, "⇉", Style::default());
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textfield_editing() {
        let mut f = TextField::default();
        f.insert_str("héllo");
        assert_eq!((f.text.as_str(), f.cursor), ("héllo", 5));
        f.left();
        f.left();
        f.backspace();
        assert_eq!(f.text, "hélo");
        f.delete();
        assert_eq!(f.text, "héo");
        f.home();
        f.insert_str("X");
        assert_eq!(f.text, "Xhéo");
        f.end();
        f.kill_to_start();
        assert_eq!((f.text.as_str(), f.cursor), ("", 0));
        f.insert_str("a\nb");
        assert_eq!(f.text, "ab", "control chars are dropped");
    }

    #[test]
    fn layout_is_top_right_and_buttons_do_not_overlap() {
        let area = Rect::new(0, 0, 100, 30);
        let l = bar_layout(area, false);
        assert_eq!(l.rect.y, 0);
        assert!(l.rect.right() <= area.right());
        assert!(l.rect.x > area.width / 2, "bar sits in the right half");
        let all = [l.input, l.count, l.case_btn, l.word_btn, l.regex_btn, l.prev_btn, l.next_btn, l.close_btn];
        for (i, a) in all.iter().enumerate() {
            assert!(contains(l.rect, a.x, a.y) && a.right() <= l.rect.right(), "{a:?} outside bar");
            for b in &all[i + 1..] {
                assert!(a.right() <= b.x || b.right() <= a.x, "{a:?} overlaps {b:?}");
            }
        }
        let r = bar_layout(area, true);
        assert_eq!(r.rect.height, 4);
        assert!(r.repl_input.is_some() && r.repl_one.is_some() && r.repl_all.is_some());
    }

    #[test]
    fn layout_survives_tiny_areas() {
        let _ = bar_layout(Rect::new(0, 0, 10, 2), true);
        let _ = bar_layout(Rect::new(0, 0, 0, 0), false);
    }

    #[test]
    fn matches_refresh_and_label() {
        let mut b = Buffer::from_str("foo bar foo baz foo");
        let mut bar = FindBar::new(&FindConfig::default());
        bar.open = true;
        bar.query.set("foo");
        bar.refresh(&b);
        assert_eq!(bar.matches.len(), 3);
        assert_eq!(bar.count_label(&b), "? of 3");
        b.anchor = Some(8);
        b.cursor = 11;
        assert_eq!(bar.count_label(&b), "2 of 3");
        bar.query.set("zzz");
        bar.refresh(&b);
        assert_eq!(bar.count_label(&b), "No results");
        bar.regex = true;
        bar.query.set("(");
        bar.refresh(&b);
        assert_eq!(bar.count_label(&b), "Bad regex");
        bar.query.clear();
        bar.refresh(&b);
        assert_eq!(bar.count_label(&b), "");
    }

    #[test]
    fn matches_follow_buffer_edits() {
        let mut b = Buffer::from_str("a a");
        let mut bar = FindBar::new(&FindConfig::default());
        bar.open = true;
        bar.query.set("a");
        bar.refresh(&b);
        assert_eq!(bar.matches.len(), 2);
        b.insert(3, " a");
        bar.refresh(&b);
        assert_eq!(bar.matches.len(), 3);
    }
}
