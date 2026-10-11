//! Text buffer: rope + cursor + selection anchor + grouped undo/redo.
//! All positions are char indices.

use ropey::Rope;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct Edit {
    pos: usize,
    removed: String,
    inserted: String,
}

#[derive(Debug, Clone)]
struct Group {
    edits: Vec<Edit>,
}

#[derive(Debug)]
pub struct Buffer {
    rope: Rope,
    pub cursor: usize,
    pub anchor: Option<usize>,
    /// Remembered column for vertical motion.
    pub goal_col: usize,
    undo: Vec<Group>,
    redo: Vec<Group>,
    open: bool,
    pub version: u64,
    saved_version: u64,
    pub path: Option<PathBuf>,
    pub crlf: bool,
}

pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[derive(PartialEq)]
enum Class {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if is_word_char(c) {
        Class::Word
    } else {
        Class::Punct
    }
}

impl Buffer {
    pub fn new() -> Self {
        Self::from_str("")
    }

    #[allow(clippy::should_implement_trait)] // infallible, unlike FromStr
    pub fn from_str(text: &str) -> Self {
        let crlf = text.contains("\r\n");
        let norm = if crlf { text.replace("\r\n", "\n") } else { text.to_string() };
        Buffer {
            rope: Rope::from_str(&norm),
            cursor: 0,
            anchor: None,
            goal_col: 0,
            undo: vec![],
            redo: vec![],
            open: false,
            version: 0,
            saved_version: 0,
            path: None,
            crlf,
        }
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let mut b = Buffer::from_str(&String::from_utf8_lossy(&bytes));
        b.path = Some(path.to_path_buf());
        Ok(b)
    }

    pub fn save(&mut self) -> std::io::Result<usize> {
        let path = self.path.clone().ok_or_else(|| std::io::Error::other("no file name"))?;
        self.save_as(&path)
    }

    pub fn save_as(&mut self, path: &Path) -> std::io::Result<usize> {
        let mut text = self.text();
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        std::fs::write(path, text)?;
        self.path = Some(path.to_path_buf());
        self.saved_version = self.version;
        Ok(self.line_count())
    }

    pub fn is_modified(&self) -> bool {
        self.version != self.saved_version
    }

    // ---- reading -------------------------------------------------------

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Number of lines; a trailing newline does not add a phantom last line
    /// for display purposes except when the buffer is empty.
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line(&self, i: usize) -> String {
        if i >= self.line_count() {
            return String::new();
        }
        let mut s = self.rope.line(i).to_string();
        while s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
        s
    }

    pub fn line_len(&self, i: usize) -> usize {
        self.line(i).chars().count()
    }

    pub fn line_start(&self, i: usize) -> usize {
        self.rope.line_to_char(i.min(self.line_count() - 1))
    }

    /// Char index just past the last char of the line (before the newline).
    pub fn line_end(&self, i: usize) -> usize {
        self.line_start(i) + self.line_len(i)
    }

    pub fn char_at(&self, idx: usize) -> Option<char> {
        (idx < self.len_chars()).then(|| self.rope.char(idx))
    }

    pub fn slice(&self, r: Range<usize>) -> String {
        self.rope.slice(r.start.min(self.len_chars())..r.end.min(self.len_chars())).to_string()
    }

    pub fn line_of(&self, idx: usize) -> usize {
        self.rope.char_to_line(idx.min(self.len_chars()))
    }

    pub fn line_col(&self, idx: usize) -> (usize, usize) {
        let l = self.line_of(idx);
        (l, idx.min(self.len_chars()) - self.line_start(l))
    }

    pub fn pos_of(&self, line: usize, col: usize) -> usize {
        let l = line.min(self.line_count() - 1);
        self.line_start(l) + col.min(self.line_len(l))
    }

    pub fn cursor_line_col(&self) -> (usize, usize) {
        self.line_col(self.cursor)
    }

    // ---- editing -------------------------------------------------------

    fn record(&mut self, edit: Edit) {
        self.redo.clear();
        match (self.open, self.undo.last_mut()) {
            (true, Some(g)) => g.edits.push(edit),
            _ => self.undo.push(Group { edits: vec![edit] }),
        }
        self.open = true;
        self.version += 1;
    }

    /// Next edit starts a new undo step.
    pub fn break_group(&mut self) {
        self.open = false;
    }

    pub fn insert(&mut self, pos: usize, s: &str) {
        if s.is_empty() {
            return;
        }
        let pos = pos.min(self.len_chars());
        self.rope.insert(pos, s);
        self.record(Edit { pos, removed: String::new(), inserted: s.to_string() });
    }

    pub fn remove(&mut self, r: Range<usize>) -> String {
        let r = r.start.min(self.len_chars())..r.end.min(self.len_chars());
        if r.start >= r.end {
            return String::new();
        }
        let removed = self.rope.slice(r.clone()).to_string();
        self.rope.remove(r.clone());
        self.record(Edit { pos: r.start, removed: removed.clone(), inserted: String::new() });
        removed
    }

    pub fn replace(&mut self, r: Range<usize>, s: &str) {
        let start = r.start;
        self.remove(r);
        self.insert(start, s);
    }

    /// Replace the whole text in one undo step (used by replace-all).
    pub fn set_text_undoable(&mut self, text: &str) {
        let n = self.len_chars();
        self.break_group();
        self.replace(0..n, text);
        self.break_group();
        self.cursor = self.cursor.min(self.len_chars());
        self.anchor = None;
    }

    pub fn insert_at_cursor(&mut self, s: &str) {
        let c = self.cursor;
        self.insert(c, s);
        self.cursor = c + s.chars().count();
        self.goal_col = self.cursor_line_col().1;
    }

    pub fn delete_back(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let c = self.cursor;
            self.remove(c..c + 1);
        }
    }

    pub fn delete_forward(&mut self) {
        let c = self.cursor;
        self.remove(c..c + 1);
    }

    pub fn undo(&mut self) -> bool {
        self.open = false;
        let Some(group) = self.undo.pop() else { return false };
        let mut cursor = 0;
        for e in group.edits.iter().rev() {
            if !e.inserted.is_empty() {
                self.rope.remove(e.pos..e.pos + e.inserted.chars().count());
            }
            if !e.removed.is_empty() {
                self.rope.insert(e.pos, &e.removed);
            }
            cursor = e.pos;
        }
        self.redo.push(group);
        self.version += 1;
        self.cursor = cursor.min(self.len_chars());
        self.anchor = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        self.open = false;
        let Some(group) = self.redo.pop() else { return false };
        let mut cursor = 0;
        for e in &group.edits {
            if !e.removed.is_empty() {
                self.rope.remove(e.pos..e.pos + e.removed.chars().count());
            }
            if !e.inserted.is_empty() {
                self.rope.insert(e.pos, &e.inserted);
            }
            cursor = e.pos + e.inserted.chars().count();
        }
        self.undo.push(group);
        self.version += 1;
        self.cursor = cursor.min(self.len_chars());
        self.anchor = None;
        true
    }

    // ---- selection -----------------------------------------------------

    /// Exclusive selection range (anchor..cursor, ordered); None when empty.
    pub fn selection(&self) -> Option<Range<usize>> {
        let a = self.anchor?;
        if a == self.cursor {
            return None;
        }
        Some(a.min(self.cursor)..a.max(self.cursor))
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|r| self.slice(r))
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.len_chars();
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Delete the selection; returns the removed text.
    pub fn delete_selection(&mut self) -> Option<String> {
        let r = self.selection()?;
        self.cursor = r.start;
        self.anchor = None;
        Some(self.remove(r))
    }

    pub fn word_range_at(&self, pos: usize) -> Range<usize> {
        let (l, _) = self.line_col(pos);
        let (s, e) = (self.line_start(l), self.line_end(l));
        if s == e {
            return s..s;
        }
        let pos = pos.min(e.saturating_sub(1)).max(s);
        let cls = |i: usize| class(self.rope.char(i));
        let base = cls(pos);
        let mut a = pos;
        while a > s && cls(a - 1) == base {
            a -= 1;
        }
        let mut b = pos + 1;
        while b < e && cls(b) == base {
            b += 1;
        }
        a..b
    }

    /// Whole line including its trailing newline (if any).
    pub fn line_range(&self, line: usize) -> Range<usize> {
        let l = line.min(self.line_count() - 1);
        let start = self.line_start(l);
        let end = if l + 1 < self.line_count() { self.line_start(l + 1) } else { self.len_chars() };
        start..end
    }

    // ---- motions (return targets; callers move the cursor) ---------------

    pub fn left(&self, pos: usize) -> usize {
        let (l, c) = self.line_col(pos);
        if c > 0 {
            pos - 1
        } else if l > 0 {
            self.line_end(l - 1)
        } else {
            0
        }
    }

    pub fn right(&self, pos: usize) -> usize {
        if pos < self.len_chars() { pos + 1 } else { pos }
    }

    pub fn up(&self, pos: usize, goal: usize) -> usize {
        let l = self.line_of(pos);
        if l == 0 { 0 } else { self.pos_of(l - 1, goal) }
    }

    pub fn down(&self, pos: usize, goal: usize) -> usize {
        let l = self.line_of(pos);
        if l + 1 >= self.line_count() { self.line_end(l) } else { self.pos_of(l + 1, goal) }
    }

    pub fn word_forward(&self, pos: usize) -> usize {
        let n = self.len_chars();
        let mut i = pos;
        if i >= n {
            return n;
        }
        let base = class(self.rope.char(i));
        while i < n && base != Class::Space && class(self.rope.char(i)) == base {
            i += 1;
        }
        while i < n && class(self.rope.char(i)) == Class::Space {
            i += 1;
        }
        i
    }

    pub fn word_back(&self, pos: usize) -> usize {
        let mut i = pos;
        while i > 0 && class(self.rope.char(i - 1)) == Class::Space {
            i -= 1;
        }
        if i == 0 {
            return 0;
        }
        let base = class(self.rope.char(i - 1));
        while i > 0 && class(self.rope.char(i - 1)) == base {
            i -= 1;
        }
        i
    }

    /// Last char of the current/next word (vi `e`).
    pub fn word_end(&self, pos: usize) -> usize {
        let n = self.len_chars();
        let mut i = pos + 1;
        while i < n && class(self.rope.char(i)) == Class::Space {
            i += 1;
        }
        if i >= n {
            return n.saturating_sub(1);
        }
        let base = class(self.rope.char(i));
        while i + 1 < n && class(self.rope.char(i + 1)) == base {
            i += 1;
        }
        i
    }

    pub fn first_non_blank(&self, line: usize) -> usize {
        let s = self.line_start(line);
        let off = self.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').count();
        s + off
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Buffer {
        Buffer::from_str(s)
    }

    #[test]
    fn insert_and_delete() {
        let mut x = b("hello");
        x.insert(5, " world");
        assert_eq!(x.text(), "hello world");
        assert_eq!(x.remove(0..6), "hello ");
        assert_eq!(x.text(), "world");
    }

    #[test]
    fn insert_at_cursor_moves_cursor() {
        let mut x = b("ac");
        x.cursor = 1;
        x.insert_at_cursor("b");
        assert_eq!((x.text().as_str(), x.cursor), ("abc", 2));
    }

    #[test]
    fn backspace_and_delete_forward() {
        let mut x = b("abc");
        x.cursor = 2;
        x.delete_back();
        assert_eq!(x.text(), "ac");
        x.delete_forward();
        assert_eq!(x.text(), "a");
        x.cursor = 0;
        x.delete_back(); // no-op at start
        assert_eq!(x.text(), "a");
    }

    #[test]
    fn unicode_editing_uses_chars() {
        let mut x = b("é😀z");
        assert_eq!(x.len_chars(), 3);
        x.cursor = 2;
        x.delete_back();
        assert_eq!(x.text(), "éz");
    }

    #[test]
    fn line_helpers() {
        let x = b("one\ntwo\n\nfour");
        assert_eq!(x.line_count(), 4);
        assert_eq!(x.line(1), "two");
        assert_eq!(x.line_start(3), 9);
        assert_eq!(x.line_col(5), (1, 1));
        assert_eq!(x.pos_of(1, 99), 7, "col clamps to line end");
        assert_eq!(x.line_range(0), 0..4);
        assert_eq!(x.line_range(3), 9..13, "last line has no newline");
    }

    #[test]
    fn crlf_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, "a\r\nb\r\n").unwrap();
        let mut x = Buffer::load(&p).unwrap();
        assert_eq!(x.text(), "a\nb\n");
        x.insert(1, "!");
        x.save().unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"a!\r\nb\r\n");
        assert!(!x.is_modified());
    }

    #[test]
    fn modified_flag_follows_undo_state() {
        let mut x = b("a");
        assert!(!x.is_modified());
        x.insert(1, "b");
        assert!(x.is_modified());
    }

    #[test]
    fn undo_redo_basic() {
        let mut x = b("abc");
        x.insert(3, "d");
        x.break_group();
        x.remove(0..1);
        assert_eq!(x.text(), "bcd");
        assert!(x.undo());
        assert_eq!(x.text(), "abcd");
        assert!(x.undo());
        assert_eq!(x.text(), "abc");
        assert!(!x.undo());
        assert!(x.redo());
        assert!(x.redo());
        assert_eq!(x.text(), "bcd");
        assert!(!x.redo());
    }

    #[test]
    fn typing_coalesces_until_break() {
        let mut x = b("");
        for c in "abc".chars() {
            x.insert_at_cursor(&c.to_string());
        }
        x.break_group();
        x.insert_at_cursor("d");
        x.undo();
        assert_eq!(x.text(), "abc");
        x.undo();
        assert_eq!(x.text(), "");
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut x = b("a");
        x.insert(1, "b");
        x.undo();
        x.break_group();
        x.insert(1, "c");
        assert!(!x.redo());
        assert_eq!(x.text(), "ac");
    }

    #[test]
    fn undo_restores_cursor_at_edit() {
        let mut x = b("hello world");
        x.break_group();
        x.remove(5..11);
        x.undo();
        assert_eq!(x.text(), "hello world");
        assert_eq!(x.cursor, 5);
    }

    #[test]
    fn selection_and_delete() {
        let mut x = b("hello world");
        x.anchor = Some(6);
        x.cursor = 11;
        assert_eq!(x.selection(), Some(6..11));
        assert_eq!(x.selected_text().as_deref(), Some("world"));
        assert_eq!(x.delete_selection().as_deref(), Some("world"));
        assert_eq!(x.text(), "hello ");
        assert_eq!(x.cursor, 6);
        assert!(x.selection().is_none());
    }

    #[test]
    fn backwards_selection_is_ordered() {
        let mut x = b("abcdef");
        x.anchor = Some(4);
        x.cursor = 1;
        assert_eq!(x.selected_text().as_deref(), Some("bcd"));
    }

    #[test]
    fn empty_selection_is_none() {
        let mut x = b("abc");
        x.anchor = Some(1);
        x.cursor = 1;
        assert!(x.selection().is_none());
    }

    #[test]
    fn select_all() {
        let mut x = b("a\nb");
        x.select_all();
        assert_eq!(x.selected_text().as_deref(), Some("a\nb"));
    }

    #[test]
    fn word_range() {
        let x = b("foo bar_baz  +-");
        assert_eq!(x.word_range_at(1), 0..3);
        assert_eq!(x.word_range_at(5), 4..11);
        assert_eq!(x.word_range_at(12), 11..13, "whitespace run");
        assert_eq!(x.word_range_at(13), 13..15, "punctuation run");
    }

    #[test]
    fn word_range_on_empty_line() {
        let x = b("a\n\nb");
        assert_eq!(x.word_range_at(2), 2..2);
    }

    #[test]
    fn vi_word_motions() {
        let x = b("foo bar.baz");
        assert_eq!(x.word_forward(0), 4);
        assert_eq!(x.word_forward(4), 7);
        assert_eq!(x.word_back(7), 4);
        assert_eq!(x.word_back(4), 0);
        assert_eq!(x.word_end(0), 2);
        assert_eq!(x.word_end(2), 6);
    }

    #[test]
    fn vertical_motion_keeps_goal_column() {
        let x = b("long line\nab\nlong line");
        let p = x.down(6, 6);
        assert_eq!(p, x.line_end(1));
        assert_eq!(x.down(p, 6), x.pos_of(2, 6));
        assert_eq!(x.up(x.pos_of(2, 6), 6), x.line_end(1));
    }

    #[test]
    fn set_text_undoable_is_one_step() {
        let mut x = b("aaa bbb");
        x.set_text_undoable("xxx");
        assert_eq!(x.text(), "xxx");
        x.undo();
        assert_eq!(x.text(), "aaa bbb");
    }
}
