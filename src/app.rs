//! Editor state and input handling (keyboard, mouse, `:` commands, SQL glue).
//! Drawing lives in `render.rs`; both operate on `App`.

use crate::buffer::Buffer;
use crate::clipboard::Clipboard;
use crate::config::Config;
use crate::find::{next_match, prev_match};
use crate::findbar::{self, FindBar, TextField, contains};
use crate::indent::{self, Indent};
use crate::keyspec::{self, Chord};
use crate::picker::{self, Picker, PickerItem, PickerKind};
use crate::sql::{self, DbError, ResultSet, Session, profile, stmt};
use crate::suggest::{self, Suggestions};
use crate::syntax::{self, Kind, LangSpec};
use crate::table::{self, TableOpts, TableState};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Editor,
    Find,
    Table,
    Prompt,
    Confirm,
    Picker,
}

#[derive(Clone, PartialEq, Debug)]
pub enum PromptKind {
    Command,
    Password(String),
}

#[derive(Clone, Debug)]
pub struct Prompt {
    pub kind: PromptKind,
    pub field: TextField,
}

pub struct ResultView {
    pub outcome: Result<ResultSet, DbError>,
    pub elapsed: Duration,
    pub status: String,
    pub state: TableState,
    /// Column widths needed to show every cell, computed once (scanning a big result on every key press would be slow).
    pub natural: Vec<usize>,
}

impl ResultView {
    pub fn new(outcome: Result<ResultSet, DbError>, elapsed: Duration, status: String) -> ResultView {
        let natural = match &outcome {
            Ok(rs) => table::natural_widths(&rs.columns, &rs.rows),
            Err(_) => vec![],
        };
        ResultView { outcome, elapsed, status, state: TableState::default(), natural }
    }
}

#[derive(Default)]
pub struct SqlUi {
    pub session: Option<Session>,
    pub connecting: Option<(String, Receiver<Result<Session, DbError>>)>,
    pub result: Option<ResultView>,
    pub pane_open: bool,
    /// With `result_view = "tab"`: the results tab is currently in front of the editor.
    pub tab_active: bool,
    /// 0-based (line, col) in the buffer where the executed text started.
    pub origin: (usize, usize),
}

pub struct Layout {
    pub text: Rect,
    pub gutter_w: u16,
    pub pane: Option<Rect>,
    pub status: Rect,
    pub cmd: Rect,
}

struct Click {
    at: Instant,
    pos: (u16, u16),
    count: u8,
}

/// The word-suggestion popup.
#[derive(Debug, Clone)]
pub struct SuggestState {
    pub sug: Suggestions,
    pub selected: usize,
}

pub struct SynCache {
    version: u64,
    lang: &'static str,
    pub kinds: Vec<Vec<Kind>>,
}

pub struct App {
    pub cfg: Config,
    pub buffer: Buffer,
    pub mode: Mode,
    pub focus: Focus,
    pub find: FindBar,
    pub prompt: Option<Prompt>,
    pub confirm: Option<String>,
    pub clip: Box<dyn Clipboard>,
    pub top_line: usize,
    pub left_col: usize,
    pub follow: bool,
    pub msg: Option<String>,
    pub register: Option<(String, bool)>,
    count: Option<usize>,
    op: Option<char>,
    g_pending: bool,
    line_copy: Option<String>,
    click: Option<Click>,
    drag_origin: Option<usize>,
    pub area: Rect,
    /// Time source for click timing; tests replace it with a manual clock.
    clock: std::sync::Arc<dyn Fn() -> Instant + Send + Sync>,
    pub quit: bool,
    pub syn: Option<SynCache>,
    pub lang: &'static LangSpec,
    /// Set by `:setlang`; overrides detection by extension until `:setlang auto`.
    pub lang_override: Option<&'static LangSpec>,
    pub picker: Option<Picker>,
    /// Does the terminal speak the kitty keyboard protocol? `None` until detected (tests, startup).
    pub kitty: Option<bool>,
    kitty_notice_shown: bool,
    run_fallback: Chord,
    run_line_key: Chord,
    pub suggest: Option<SuggestState>,
    /// Buffer version at which the user pressed Esc on the popup (it stays closed until the next edit).
    suggest_dismissed: Option<u64>,
    /// Buffer version the popup was last evaluated at: it only *opens* because of an edit (typing).
    suggest_seen: u64,
    pub sql: SqlUi,
    pub warnings: Vec<String>,
}

pub const DISCONNECTED_MSG: &str = "Not connected to a database. Use :connect <profile>";

fn first_line(b: &Buffer) -> String {
    b.line(0)
}

/// Display column of char `col` in `line`, expanding tabs.
pub fn cell_x(line: &str, col: usize, tab: usize) -> usize {
    let mut x = 0;
    for c in line.chars().take(col) {
        x += char_cells(c, x, tab);
    }
    x
}

fn char_cells(c: char, x: usize, tab: usize) -> usize {
    use unicode_width::UnicodeWidthChar;
    if c == '\t' {
        tab - x % tab
    } else if c.is_control() {
        1
    } else {
        UnicodeWidthChar::width(c).unwrap_or(0)
    }
}

/// Char column whose cells cover display column `x` (clamped to the line end).
pub fn col_at_x(line: &str, x: usize, tab: usize) -> usize {
    let mut cx = 0;
    for (i, c) in line.chars().enumerate() {
        let w = char_cells(c, cx, tab);
        if x < cx + w.max(1) {
            return i;
        }
        cx += w;
    }
    line.chars().count()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordFwd,
    WordBack,
    WordEnd,
    LineStart,
    LineEnd,
    FirstNonBlank,
    FileStart,
    FileEnd,
}

struct Target {
    pos: usize,
    linewise: bool,
    inclusive: bool,
}

impl App {
    pub fn new(cfg: Config, buffer: Buffer, clip: Box<dyn Clipboard>) -> App {
        let lang = syntax::detect(buffer.path.as_deref(), &first_line(&buffer));
        let mode = if cfg.editor.modal { Mode::Normal } else { Mode::Insert };
        let find = FindBar::new(&cfg.find);
        let default_chord = |spec: &str| keyspec::parse(spec).expect("built-in key spec");
        let run_fallback = keyspec::parse(&cfg.sql.run_fallback_key).unwrap_or_else(|| default_chord("alt+enter"));
        let run_line_key = keyspec::parse(&cfg.sql.run_line_key).unwrap_or_else(|| default_chord("alt+l"));
        App {
            cfg,
            buffer,
            mode,
            focus: Focus::Editor,
            find,
            prompt: None,
            confirm: None,
            clip,
            top_line: 0,
            left_col: 0,
            follow: true,
            msg: None,
            register: None,
            count: None,
            op: None,
            g_pending: false,
            line_copy: None,
            click: None,
            drag_origin: None,
            area: Rect::new(0, 0, 80, 24),
            clock: std::sync::Arc::new(Instant::now),
            quit: false,
            syn: None,
            lang,
            lang_override: None,
            picker: None,
            kitty: None,
            kitty_notice_shown: false,
            run_fallback,
            run_line_key,
            suggest: None,
            suggest_dismissed: None,
            suggest_seen: 0,
            sql: SqlUi::default(),
            warnings: vec![],
        }
    }

    /// Re-derive the language: the manual `:setlang` choice wins over detection by extension.
    pub fn update_lang(&mut self) {
        self.lang = self.lang_override.unwrap_or_else(|| syntax::detect(self.buffer.path.as_deref(), &first_line(&self.buffer)));
        self.maybe_kitty_notice();
    }

    /// True while edits are refused: a SQL buffer with no database connection
    /// (see `sql.block_editing_when_disconnected`).
    pub fn edit_blocked(&self) -> bool {
        self.cfg.sql.block_editing_when_disconnected && self.lang.name == syntax::SQL.name && self.sql.session.is_none()
    }

    /// Tell the user once, in a SQL buffer, that this terminal cannot send Ctrl+Enter.
    pub fn maybe_kitty_notice(&mut self) {
        if self.kitty == Some(false) && self.lang.name == syntax::SQL.name && !self.kitty_notice_shown {
            self.kitty_notice_shown = true;
            // short enough to fit an 80-column terminal; docs/KEYBINDINGS.md has the full story
            self.flash(format!("Ctrl+Enter unavailable (no kitty keyboard protocol): use {} or :run", self.run_fallback));
        }
    }

    pub fn set_kitty(&mut self, supported: bool) {
        self.kitty = Some(supported);
        self.maybe_kitty_notice();
    }

    /// Run `f` as one user action. While editing is blocked the buffer refuses changes; if the action
    /// tried to change it, undo every side effect (cursor, selection, mode) and explain why.
    fn guarded(&mut self, f: impl FnOnce(&mut Self)) {
        let saved = (self.buffer.cursor, self.buffer.anchor, self.buffer.goal_col, self.mode);
        self.buffer.read_only = self.edit_blocked();
        self.buffer.blocked_attempt = false;
        f(self);
        let attempted = self.buffer.blocked_attempt;
        self.buffer.read_only = false;
        self.buffer.blocked_attempt = false;
        if attempted {
            (self.buffer.cursor, self.buffer.anchor, self.buffer.goal_col, self.mode) = saved;
            self.flash(DISCONNECTED_MSG);
        }
    }

    pub fn set_lang(&mut self, mode: Option<&'static LangSpec>) {
        self.lang_override = mode;
        self.update_lang();
        self.syn = None; // recolour the whole buffer now
    }

    /// Replace the time source (double/triple-click detection). Used by tests for deterministic timing.
    pub fn set_clock(&mut self, clock: impl Fn() -> Instant + Send + Sync + 'static) {
        self.clock = std::sync::Arc::new(clock);
    }

    pub fn modal(&self) -> bool {
        self.cfg.editor.modal
    }

    pub fn mode_label(&self) -> &'static str {
        if !self.modal() {
            return "";
        }
        match self.mode {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
        }
    }

    pub fn flash(&mut self, s: impl Into<String>) {
        self.msg = Some(s.into());
    }

    // ------------------------------------------------------------------
    // geometry
    // ------------------------------------------------------------------

    pub fn layout(&self, area: Rect) -> Layout {
        let h = area.height;
        let status = Rect::new(area.x, area.bottom().saturating_sub(2).max(area.y), area.width, h.min(2).saturating_sub(1));
        let cmd = Rect::new(area.x, area.bottom().saturating_sub(1).max(area.y), area.width, h.min(1));
        let avail = h.saturating_sub(2);
        let tab_mode = self.cfg.sql.result_view == "tab";
        let mut pane_h = 0;
        if self.sql.pane_open && tab_mode {
            // results tab: replaces the editor while active, hidden otherwise
            if self.sql.tab_active && avail >= 4 {
                pane_h = avail;
            }
        } else if self.sql.pane_open && avail >= 8 {
            let body = match &self.sql.result {
                Some(r) => match &r.outcome {
                    Ok(rs) => rs.rows.len().clamp(1, self.cfg.sql.table_max_rows),
                    Err(e) => self.error_lines(e, area.width as usize).len().clamp(2, 10),
                },
                None => 2,
            } as u16;
            pane_h = (3 + body).min(avail * 3 / 5).max(4);
        }
        let text = Rect::new(area.x, area.y, area.width, avail - pane_h);
        let pane = (pane_h > 0).then(|| Rect::new(area.x, text.bottom(), area.width, pane_h));
        // width follows the digit count of the total line count: stable while scrolling, grows at 10/100/1000
        let digits = self.buffer.line_count().to_string().len() as u16;
        let gutter_w = if self.cfg.editor.line_numbers { digits + 2 } else { 0 };
        Layout { text, gutter_w, pane, status, cmd }
    }

    pub fn error_lines(&self, e: &DbError, width: usize) -> Vec<String> {
        let mut head = e.category.label().to_string();
        if let Some(l) = e.line {
            head.push_str(&format!(" at line {l}"));
            if let Some(c) = e.column {
                head.push_str(&format!(", col {c}"));
            }
        }
        if let Some(code) = &e.code {
            head.push_str(&format!("  [{code}]"));
        }
        let mut out = vec![head];
        for raw in e.message.lines() {
            let mut cur = String::new();
            for word in raw.split_whitespace() {
                if !cur.is_empty() && table::display_width(&cur) + 1 + table::display_width(word) > width.max(10) {
                    out.push(std::mem::take(&mut cur));
                }
                if !cur.is_empty() {
                    cur.push(' ');
                }
                cur.push_str(word);
            }
            out.push(cur);
        }
        out
    }

    pub fn table_opts(&self) -> TableOpts {
        TableOpts {
            max_width: self.cfg.sql.table_max_width,
            max_rows: self.cfg.sql.table_max_rows,
            column_max_width: self.cfg.sql.column_max_width,
        }
    }

    /// (viewport width, body rows) of the table pane.
    pub fn table_viewport(&self) -> Option<(usize, usize)> {
        let p = self.layout(self.area).pane?;
        Some((p.width as usize, p.height.saturating_sub(3) as usize))
    }

    // ------------------------------------------------------------------
    // selection helpers
    // ------------------------------------------------------------------

    /// Active selection as an exclusive char range. Visual modes are inclusive of the cursor char.
    pub fn selection_range(&self) -> Option<Range<usize>> {
        let b = &self.buffer;
        match self.mode {
            Mode::Visual => {
                let a = b.anchor.unwrap_or(b.cursor);
                let (s, e) = (a.min(b.cursor), a.max(b.cursor));
                Some(s..(e + 1).min(b.len_chars()).max(s))
            }
            Mode::VisualLine => {
                let a = b.anchor.unwrap_or(b.cursor);
                let (l1, l2) = (b.line_of(a.min(b.cursor)), b.line_of(a.max(b.cursor)));
                Some(b.line_range(l1).start..b.line_range(l2).end)
            }
            _ => b.selection(),
        }
    }

    fn clamp_normal(&mut self) {
        if self.modal() && self.mode == Mode::Normal && self.buffer.anchor.is_none() {
            let (l, _) = self.buffer.cursor_line_col();
            let len = self.buffer.line_len(l);
            let end = self.buffer.line_end(l);
            if len > 0 && self.buffer.cursor >= end {
                self.buffer.cursor = end - 1;
            }
        }
    }

    fn set_mode(&mut self, m: Mode) {
        if !self.modal() {
            return;
        }
        if m == Mode::Insert && self.edit_blocked() {
            return self.flash(DISCONNECTED_MSG);
        }
        self.buffer.break_group();
        self.mode = m;
    }

    fn move_to(&mut self, pos: usize, extend: bool, keep_goal: bool) {
        self.buffer.break_group();
        if extend {
            if self.buffer.anchor.is_none() {
                self.buffer.anchor = Some(self.buffer.cursor);
            }
        } else if !matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            self.buffer.anchor = None;
        }
        self.buffer.cursor = pos.min(self.buffer.len_chars());
        if !keep_goal {
            self.buffer.goal_col = self.buffer.cursor_line_col().1;
        }
        self.clamp_normal();
    }

    // ------------------------------------------------------------------
    // clipboard
    // ------------------------------------------------------------------

    fn set_clipboard(&mut self, text: &str) {
        if let Err(e) = self.clip.set(text) {
            self.flash(format!("clipboard: {e}"));
        }
    }

    pub fn copy(&mut self) {
        if let Some(r) = self.selection_range() {
            let t = self.buffer.slice(r);
            self.line_copy = None;
            self.set_clipboard(&t);
            self.flash(format!("copied {} chars", t.chars().count()));
        } else {
            // like VS Code: no selection copies the whole line
            let l = self.buffer.cursor_line_col().0;
            let mut t = self.buffer.slice(self.buffer.line_range(l));
            if !t.ends_with('\n') {
                t.push('\n');
            }
            self.line_copy = Some(t.clone());
            self.set_clipboard(&t);
            self.flash("copied line");
        }
    }

    pub fn cut(&mut self) {
        self.buffer.break_group();
        if let Some(r) = self.selection_range() {
            let t = self.buffer.slice(r.clone());
            self.line_copy = None;
            self.set_clipboard(&t);
            self.buffer.remove(r.clone());
            self.buffer.cursor = r.start;
            self.leave_visual();
        } else {
            self.copy();
            let l = self.buffer.cursor_line_col().0;
            let mut r = self.buffer.line_range(l);
            if r.end == self.buffer.len_chars() && r.start > 0 {
                r.start -= 1;
            }
            self.buffer.cursor = r.start.min(self.buffer.len_chars());
            self.buffer.remove(r);
            let l = self.buffer.cursor_line_col().0;
            self.buffer.cursor = self.buffer.first_non_blank(l);
        }
        self.buffer.anchor = None;
        self.buffer.break_group();
        self.clamp_normal();
    }

    pub fn paste(&mut self) {
        let text = match self.clip.get() {
            Ok(t) if !t.is_empty() => t.replace("\r\n", "\n"),
            Ok(_) => return,
            Err(e) => return self.flash(format!("clipboard: {e}")),
        };
        self.paste_text(&text);
    }

    pub fn paste_text(&mut self, text: &str) {
        self.buffer.break_group();
        if let Some(r) = self.selection_range() {
            self.buffer.remove(r.clone());
            self.buffer.cursor = r.start;
            self.leave_visual();
            self.buffer.anchor = None;
        } else if self.line_copy.as_deref() == Some(text) {
            // whole-line paste goes above the current line, cursor keeps its column
            let (l, c) = self.buffer.cursor_line_col();
            let at = self.buffer.line_start(l);
            self.buffer.insert(at, text);
            self.buffer.cursor = self.buffer.pos_of(l + 1, c);
            self.buffer.break_group();
            return;
        }
        self.buffer.insert_at_cursor(text);
        self.buffer.break_group();
        self.clamp_normal();
    }

    fn leave_visual(&mut self) {
        if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            self.mode = Mode::Normal;
        }
    }

    // ------------------------------------------------------------------
    // keyboard
    // ------------------------------------------------------------------

    pub fn handle_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        self.guarded(|a| {
            if !a.suggest_key(&k) {
                a.handle_key_inner(k);
            }
        });
        self.refresh_suggestions();
    }

    fn handle_key_inner(&mut self, mut k: KeyEvent) {
        // legacy terminals send BackTab, kitty-protocol terminals send Tab+Shift: treat both as Shift+Tab
        if k.code == KeyCode::Tab && k.modifiers.contains(KeyModifiers::SHIFT) {
            k.code = KeyCode::BackTab;
        }
        self.msg = None;
        self.follow = true;
        match self.focus {
            Focus::Prompt => return self.prompt_key(k),
            Focus::Confirm => return self.confirm_key(k),
            Focus::Picker => return self.picker_key(k),
            _ => {}
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);

        // SQL run keys: Ctrl+Enter (needs the kitty keyboard protocol), the configurable fallback, and run-line
        if (k.code == KeyCode::Enter && ctrl && !alt) || self.run_fallback.matches(&k) {
            return self.sql_run_smart();
        }
        if self.run_line_key.matches(&k) {
            return self.sql_run_line();
        }

        if ctrl
            && !alt
            && let KeyCode::Char(c) = k.code
        {
            let in_field = self.focus == Focus::Find;
            match c.to_ascii_lowercase() {
                's' => return self.save(),
                'q' => return self.request_quit(),
                'f' => return self.open_find(false),
                'h' => return self.open_find(true),
                'z' if shift => return self.redo(),
                'z' => return self.undo(),
                'y' => return self.redo(),
                'p' => return self.open_prompt(""),
                'v' if in_field => return self.paste_into_field(),
                'c' | 'x' | 'v' | 'a' if !in_field && self.focus == Focus::Editor => {
                    match c.to_ascii_lowercase() {
                        'c' => self.copy(),
                        'x' => self.cut(),
                        'v' => self.paste(),
                        _ => {
                            self.buffer.select_all();
                            self.mode = if self.modal() && self.mode != Mode::Insert { Mode::Normal } else { self.mode };
                        }
                    }
                    return;
                }
                'c' if self.focus == Focus::Table => return self.copy_table(),
                _ => {}
            }
        }
        // Alt+C/X/V: alternatives for terminals/tmux setups where Ctrl+C/X/V are intercepted
        if alt
            && !ctrl
            && self.focus == Focus::Editor
            && let KeyCode::Char(c) = k.code
        {
            match c.to_ascii_lowercase() {
                'c' => return self.copy(),
                'x' => return self.cut(),
                'v' => return self.paste(),
                's' => return self.open_snippets(),
                _ => {}
            }
        }
        match k.code {
            KeyCode::F(3) => return self.find_step(!shift),
            KeyCode::F(4) => return self.toggle_table_focus(),
            KeyCode::F(6) => return self.sql_run_buffer(),
            KeyCode::F(7) => return self.sql_cancel(),
            _ => {}
        }
        match self.focus {
            Focus::Find => self.find_key(k),
            Focus::Table => self.table_key(k),
            _ => self.editor_key(k),
        }
    }

    pub fn handle_paste(&mut self, text: &str) {
        self.guarded(|a| a.handle_paste_inner(text));
        self.refresh_suggestions();
    }

    fn handle_paste_inner(&mut self, text: &str) {
        match self.focus {
            Focus::Find => self.paste_into_field_text(text),
            Focus::Prompt => {
                if let Some(p) = &mut self.prompt {
                    p.field.insert_str(text);
                }
            }
            Focus::Editor => {
                self.paste_text(&text.replace("\r\n", "\n").replace('\r', "\n"));
                self.follow = true;
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------
    // word suggestions
    // ------------------------------------------------------------------

    /// Recompute the popup after any input: it exists only while typing in a word in the editor.
    pub fn refresh_suggestions(&mut self) {
        let want = self.cfg.editor.word_suggestions
            && self.focus == Focus::Editor
            && (self.mode == Mode::Insert || !self.modal())
            && self.buffer.selection().is_none()
            && self.suggest_dismissed != Some(self.buffer.version);
        let edited = self.buffer.version != self.suggest_seen;
        self.suggest_seen = self.buffer.version;
        let mut found = if want && (edited || self.suggest.is_some()) {
            suggest::suggest(&self.buffer.text(), self.buffer.cursor, self.cfg.editor.suggest_min_chars, 8)
        } else {
            None
        };
        // moving the cursor (without typing) can shrink the list, never open or move it to another word
        if !edited {
            let same_word = |f: &Suggestions| self.suggest.as_ref().is_some_and(|o| o.sug.prefix_start == f.prefix_start);
            if found.as_ref().is_some_and(|f| !same_word(f)) {
                found = None;
            }
        }
        self.suggest = found.map(|sug| {
            // keep the highlighted word while the list changes under it
            let selected = self
                .suggest
                .as_ref()
                .filter(|old| old.sug.prefix_start == sug.prefix_start)
                .and_then(|old| old.sug.items.get(old.selected))
                .and_then(|w| sug.items.iter().position(|i| i == w))
                .unwrap_or(0);
            SuggestState { sug, selected }
        });
    }

    /// Keys that belong to the popup while it is open. Returns true if the key was consumed.
    fn suggest_key(&mut self, k: &KeyEvent) -> bool {
        let Some(st) = &mut self.suggest else { return false };
        if self.focus != Focus::Editor {
            return false;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let plain = !ctrl && !k.modifiers.contains(KeyModifiers::ALT) && !k.modifiers.contains(KeyModifiers::SHIFT);
        let n = st.sug.items.len();
        match k.code {
            KeyCode::Down if plain => st.selected = (st.selected + 1) % n,
            KeyCode::Up if plain => st.selected = (st.selected + n - 1) % n,
            KeyCode::Char('n') if ctrl => st.selected = (st.selected + 1) % n,
            KeyCode::Char('p') if ctrl => st.selected = (st.selected + n - 1) % n,
            KeyCode::Tab | KeyCode::Enter if plain => self.accept_suggestion(),
            KeyCode::Esc => {
                self.suggest_dismissed = Some(self.buffer.version);
                self.suggest = None;
            }
            _ => return false,
        }
        true
    }

    /// Replace the typed prefix with the highlighted word.
    pub fn accept_suggestion(&mut self) {
        let Some(st) = self.suggest.take() else { return };
        let Some(word) = st.sug.items.get(st.selected).cloned() else { return };
        self.buffer.break_group();
        let (start, end) = (st.sug.prefix_start, self.buffer.cursor);
        self.buffer.replace(start..end, &word);
        self.buffer.cursor = start + word.chars().count();
        self.buffer.goal_col = self.buffer.cursor_line_col().1;
        self.buffer.break_group();
        self.suggest_dismissed = Some(self.buffer.version);
    }

    fn paste_into_field(&mut self) {
        if let Ok(t) = self.clip.get() {
            self.paste_into_field_text(&t);
        }
    }

    fn paste_into_field_text(&mut self, t: &str) {
        let first = t.lines().next().unwrap_or("");
        if self.find.in_replace {
            self.find.repl.insert_str(first);
        } else {
            self.find.query.insert_str(first);
            self.find_changed();
        }
    }

    fn undo(&mut self) {
        if !self.buffer.undo() {
            self.flash("already at oldest change");
        }
        self.leave_visual();
        self.clamp_normal();
    }

    fn redo(&mut self) {
        if !self.buffer.redo() {
            self.flash("already at newest change");
        }
        self.leave_visual();
        self.clamp_normal();
    }

    pub fn save(&mut self) {
        match self.buffer.path.clone() {
            None => self.open_prompt("w "),
            Some(p) => match self.buffer.save() {
                Ok(n) => self.flash(format!("\"{}\" {n}L written", p.display())),
                Err(e) => self.flash(format!("cannot write {}: {e}", p.display())),
            },
        }
    }

    fn request_quit(&mut self) {
        if self.buffer.is_modified() {
            let name = self.buffer.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "[No Name]".into());
            self.confirm = Some(format!("Save changes to {name} before quitting? [y]es / [n]o / [c]ancel"));
            self.focus = Focus::Confirm;
        } else {
            self.quit = true;
        }
    }

    fn confirm_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.confirm = None;
                self.focus = Focus::Editor;
                self.save();
                if !self.buffer.is_modified() {
                    self.quit = true;
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.confirm = None;
                self.quit = true;
            }
            KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
                self.confirm = None;
                self.focus = Focus::Editor;
            }
            _ => {}
        }
    }

    fn open_prompt(&mut self, initial: &str) {
        let mut f = TextField::default();
        f.set(initial);
        self.prompt = Some(Prompt { kind: PromptKind::Command, field: f });
        self.focus = Focus::Prompt;
    }

    fn prompt_key(&mut self, k: KeyEvent) {
        let Some(p) = &mut self.prompt else {
            self.focus = Focus::Editor;
            return;
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => {
                self.prompt = None;
                self.focus = Focus::Editor;
            }
            KeyCode::Enter => {
                let p = self.prompt.take().unwrap();
                self.focus = Focus::Editor;
                match p.kind {
                    PromptKind::Command => self.exec_command(&p.field.text),
                    PromptKind::Password(name) => self.sql_connect_with(&name, Some(p.field.text)),
                }
            }
            KeyCode::Backspace if p.field.is_empty() && p.kind == PromptKind::Command => {
                self.prompt = None;
                self.focus = Focus::Editor;
            }
            KeyCode::Backspace => p.field.backspace(),
            KeyCode::Delete => p.field.delete(),
            KeyCode::Left => p.field.left(),
            KeyCode::Right => p.field.right(),
            KeyCode::Home => p.field.home(),
            KeyCode::End => p.field.end(),
            KeyCode::Char('u') if ctrl => p.field.kill_to_start(),
            KeyCode::Char(c) if !ctrl => p.field.insert_str(&c.to_string()),
            _ => {}
        }
    }

    // ---- editor keys -----------------------------------------------------

    fn editor_key(&mut self, k: KeyEvent) {
        match self.mode {
            Mode::Insert => self.insert_key(k),
            Mode::Normal => self.normal_key(k),
            Mode::Visual | Mode::VisualLine => self.visual_key(k),
        }
    }

    /// Cursor movement keys shared by every mode. Returns true if handled.
    fn nav_key(&mut self, k: KeyEvent) -> bool {
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let extend = shift || matches!(self.mode, Mode::Visual | Mode::VisualLine);
        let b = &self.buffer;
        let (line, _) = b.cursor_line_col();
        let page = self.layout(self.area).text.height.max(2) as usize - 1;
        let (pos, vertical) = match k.code {
            KeyCode::Left if ctrl => (b.word_back(b.cursor), false),
            KeyCode::Right if ctrl => (b.word_forward(b.cursor), false),
            KeyCode::Left => (b.left(b.cursor), false),
            KeyCode::Right => (b.right(b.cursor), false),
            KeyCode::Up => (b.up(b.cursor, b.goal_col), true),
            KeyCode::Down => (b.down(b.cursor, b.goal_col), true),
            KeyCode::Home if ctrl => (0, false),
            KeyCode::End if ctrl => (b.len_chars(), false),
            KeyCode::Home => {
                let fnb = b.first_non_blank(line);
                (if b.cursor == fnb { b.line_start(line) } else { fnb }, false)
            }
            KeyCode::End => (b.line_end(line), false),
            KeyCode::PageUp => (b.pos_of(line.saturating_sub(page), b.goal_col), true),
            KeyCode::PageDown => (b.pos_of(line + page, b.goal_col), true),
            _ => return false,
        };
        self.move_to(pos, extend, vertical);
        true
    }

    fn insert_text(&mut self, s: &str) {
        if let Some(r) = self.buffer.selection() {
            self.buffer.cursor = r.start;
            self.buffer.remove(r);
            self.buffer.anchor = None;
        }
        self.buffer.insert_at_cursor(s);
    }

    fn indent(&self) -> Indent {
        Indent::from_config(&self.cfg.editor)
    }

    fn delete_selection_text(&mut self) -> bool {
        match self.buffer.selection() {
            Some(r) => {
                self.buffer.cursor = r.start;
                self.buffer.remove(r);
                self.buffer.anchor = None;
                true
            }
            None => false,
        }
    }

    /// VS Code Backspace: deletes the selection; in leading indentation removes one indent step
    /// (one tab, or spaces back to the previous stop); on a whitespace run after text removes the whole run.
    fn backspace(&mut self) {
        if self.delete_selection_text() {
            return;
        }
        let (l, c) = self.buffer.cursor_line_col();
        if c == 0 {
            return self.buffer.delete_back(); // joins with the previous line
        }
        let before: String = self.buffer.line(l).chars().take(c).collect();
        let n = indent::backspace_len(&self.indent(), &before, self.cfg.editor.smart_whitespace_delete);
        let cur = self.buffer.cursor;
        self.buffer.remove(cur - n..cur);
        self.buffer.cursor = cur - n;
    }

    /// VS Code Delete: deletes the selection; on whitespace removes the whole run; at the end of a line joins the next.
    fn delete_key(&mut self) {
        if self.delete_selection_text() {
            return;
        }
        let (l, c) = self.buffer.cursor_line_col();
        let after: String = self.buffer.line(l).chars().skip(c).collect();
        match indent::delete_len(&after, self.cfg.editor.smart_whitespace_delete) {
            0 => self.buffer.delete_forward(),
            n => {
                let cur = self.buffer.cursor;
                self.buffer.remove(cur..cur + n);
            }
        }
    }

    /// Tab / Shift+Tab, following VS Code's rules:
    /// * a selection over several lines (or one whole line) indents/outdents every touched line;
    ///   a partial single-line selection is replaced by the indent text (Tab) / outdents the line (Shift+Tab)
    /// * no selection: Tab inserts at the cursor, Shift+Tab outdents the current line
    fn tab(&mut self, shift: bool) {
        let ind = self.indent();
        let b = &self.buffer;
        if let Some(r) = b.selection() {
            let ((l1, c1), (l2, c2)) = (b.line_col(r.start), b.line_col(r.end));
            let last = if l2 > l1 && c2 == 0 { l2 - 1 } else { l2 };
            let whole_line = l1 == l2 && c1 == 0 && c2 == b.line_len(l1);
            if shift {
                return self.indent_lines(l1, last, true);
            }
            if l1 != l2 || whole_line {
                return self.indent_lines(l1, last, false);
            }
            let before: String = b.line(l1).chars().take(c1).collect();
            let text = ind.tab_text(indent::display_cols(&before, ind.tab_width));
            return self.insert_text(&text);
        }
        if shift {
            let l = self.buffer.cursor_line_col().0;
            return self.indent_lines(l, l, true);
        }
        let (l, c) = self.buffer.cursor_line_col();
        let before: String = self.buffer.line(l).chars().take(c).collect();
        let text = ind.tab_text(indent::display_cols(&before, ind.tab_width));
        self.insert_text(&text);
    }

    /// Indent or outdent whole lines `l1..=l2` as one undo step, keeping the selection on the same text.
    fn indent_lines(&mut self, l1: usize, l2: usize, outdent: bool) {
        let ind = self.indent();
        self.buffer.break_group();
        let (anchor, cursor) = (self.buffer.anchor, self.buffer.cursor);
        // (line start, chars removed, text added)
        let mut edits: Vec<(usize, usize, String)> = vec![];
        for l in l1..=l2 {
            let text = self.buffer.line(l);
            let start = self.buffer.line_start(l);
            if outdent {
                let n = indent::outdent_len(&ind, &text);
                if n > 0 {
                    edits.push((start, n, String::new()));
                }
            } else if !text.is_empty() {
                edits.push((start, 0, ind.level()));
            }
        }
        for (start, n, add) in edits.iter().rev() {
            if *n > 0 {
                self.buffer.remove(*start..*start + *n);
            }
            if !add.is_empty() {
                self.buffer.insert(*start, add);
            }
        }
        // where a position ends up after the edits (positions on an edited line shift with it)
        fn remap(edits: &[(usize, usize, String)], p: usize) -> usize {
            let mut q = p as isize;
            for (start, n, add) in edits {
                if p > *start {
                    q += if *n > 0 { -((*n).min(p - *start) as isize) } else { add.chars().count() as isize };
                }
            }
            q as usize
        }
        self.buffer.cursor = remap(&edits, cursor);
        self.buffer.anchor = anchor.map(|a| remap(&edits, a));
        self.buffer.break_group();
    }

    fn insert_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        if self.nav_key(k) {
            return;
        }
        match k.code {
            KeyCode::Esc => {
                self.buffer.break_group();
                self.buffer.anchor = None;
                if self.modal() {
                    self.mode = Mode::Normal;
                    if self.buffer.cursor_line_col().1 > 0 {
                        self.buffer.cursor -= 1;
                    }
                    self.clamp_normal();
                } else if self.find.open {
                    self.find.open = false;
                }
            }
            KeyCode::Enter => {
                let (l, c) = self.buffer.cursor_line_col();
                let indent: String = self.buffer.line(l).chars().take(c).take_while(|c| *c == ' ' || *c == '\t').collect();
                self.insert_text(&format!("\n{indent}"));
            }
            KeyCode::Backspace if alt => {
                let t = self.buffer.word_back(self.buffer.cursor);
                let c = self.buffer.cursor;
                self.buffer.remove(t..c);
                self.buffer.cursor = t;
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_key(),
            KeyCode::Tab => self.tab(false),
            KeyCode::BackTab => self.tab(true),
            KeyCode::Char(c) if !ctrl && !alt => self.insert_text(&c.to_string()),
            _ => {}
        }
    }

    fn motion_for(&self, k: &KeyEvent) -> Option<Motion> {
        Some(match k.code {
            KeyCode::Char('h') | KeyCode::Left => Motion::Left,
            KeyCode::Char('l') | KeyCode::Right => Motion::Right,
            KeyCode::Char('k') | KeyCode::Up => Motion::Up,
            KeyCode::Char('j') | KeyCode::Down => Motion::Down,
            KeyCode::Char('w') => Motion::WordFwd,
            KeyCode::Char('b') => Motion::WordBack,
            KeyCode::Char('e') => Motion::WordEnd,
            KeyCode::Char('0') | KeyCode::Home => Motion::LineStart,
            KeyCode::Char('$') | KeyCode::End => Motion::LineEnd,
            KeyCode::Char('^') => Motion::FirstNonBlank,
            KeyCode::Char('G') => Motion::FileEnd,
            _ => return None,
        })
    }

    fn resolve(&self, m: Motion, count: Option<usize>) -> Target {
        let b = &self.buffer;
        let n = count.unwrap_or(1).max(1);
        let mut pos = b.cursor;
        let (line, _) = b.line_col(pos);
        let mut t = Target { pos, linewise: false, inclusive: false };
        match m {
            Motion::Left => (0..n).for_each(|_| pos = if b.line_col(pos).1 > 0 { pos - 1 } else { pos }),
            Motion::Right => (0..n).for_each(|_| {
                let l = b.line_of(pos);
                if pos < b.line_end(l).saturating_sub(if self.mode == Mode::Normal && self.op.is_none() { 1 } else { 0 }) {
                    pos += 1
                }
            }),
            Motion::Up => {
                (0..n).for_each(|_| pos = b.up(pos, b.goal_col));
                t.linewise = true;
            }
            Motion::Down => {
                // `j` on the last line must not move; `b.down` goes to the line end there
                (0..n).for_each(|_| {
                    if b.line_of(pos) + 1 < b.line_count() {
                        pos = b.down(pos, b.goal_col)
                    }
                });
                t.linewise = true;
            }
            Motion::WordFwd => (0..n).for_each(|_| pos = b.word_forward(pos)),
            Motion::WordBack => (0..n).for_each(|_| pos = b.word_back(pos)),
            Motion::WordEnd => {
                (0..n).for_each(|_| pos = b.word_end(pos));
                t.inclusive = true;
            }
            Motion::LineStart => pos = b.line_start(line),
            Motion::LineEnd => {
                pos = b.line_end((line + n - 1).min(b.line_count() - 1));
                t.inclusive = true;
                if self.op.is_none() && pos > b.line_start(b.line_of(pos)) {
                    pos -= 1;
                    t.inclusive = false;
                }
            }
            Motion::FirstNonBlank => pos = b.first_non_blank(line),
            Motion::FileStart => {
                pos = b.first_non_blank(count.map(|c| c.saturating_sub(1)).unwrap_or(0).min(b.line_count() - 1));
                t.linewise = true;
            }
            Motion::FileEnd => {
                let l = count.map(|c| c.saturating_sub(1)).unwrap_or(b.line_count() - 1).min(b.line_count() - 1);
                pos = b.first_non_blank(l);
                t.linewise = true;
            }
        }
        t.pos = pos;
        t
    }

    fn change_word_target(&self, count: Option<usize>) -> Target {
        let b = &self.buffer;
        let same = |a: Option<char>, c: Option<char>| match (a, c) {
            (Some(a), Some(c)) => {
                !a.is_whitespace() && !c.is_whitespace() && crate::buffer::is_word_char(a) == crate::buffer::is_word_char(c)
            }
            _ => false,
        };
        let here = b.char_at(b.cursor);
        if count.unwrap_or(1) == 1 && !here.is_some_and(char::is_whitespace) && !same(here, b.char_at(b.cursor + 1)) {
            return Target { pos: b.cursor, linewise: false, inclusive: true };
        }
        self.resolve(Motion::WordEnd, count)
    }

    fn take_count(&mut self) -> Option<usize> {
        self.count.take()
    }

    fn reset_pending(&mut self) {
        self.count = None;
        self.op = None;
        self.g_pending = false;
    }

    fn yank(&mut self, text: String, linewise: bool) {
        self.set_clipboard(&text);
        self.line_copy = None;
        self.register = Some((text, linewise));
    }

    /// Range to operate on for `op` + `target` from the cursor.
    fn op_range(&self, t: &Target) -> (Range<usize>, bool) {
        let b = &self.buffer;
        if t.linewise {
            let (l1, l2) = (b.line_of(b.cursor), b.line_of(t.pos));
            let (a, z) = (l1.min(l2), l1.max(l2));
            return (b.line_range(a).start..b.line_range(z).end, true);
        }
        let (mut s, mut e) = (b.cursor.min(t.pos), b.cursor.max(t.pos));
        if t.inclusive {
            e = (e + 1).min(b.len_chars());
        } else if b.line_of(e) > b.line_of(s) && self.op.is_some() {
            // `dw` on the last word of a line stops at the end of the line
            e = b.line_end(b.line_of(s));
        }
        if s > e {
            s = e;
        }
        (s..e, false)
    }

    fn apply_op(&mut self, op: char, range: Range<usize>, linewise: bool) {
        self.buffer.break_group();
        let mut text = self.buffer.slice(range.clone());
        if linewise && !text.ends_with('\n') {
            text.push('\n');
        }
        match op {
            'y' => {
                self.yank(text, linewise);
                self.buffer.cursor = range.start;
                self.buffer.anchor = None;
            }
            'd' | 'c' => {
                self.yank(text, linewise);
                let mut r = range.clone();
                if op == 'c' && linewise {
                    // keep the (now empty) line for typing
                    if self.buffer.slice(r.clone()).ends_with('\n') {
                        r.end -= 1;
                    }
                } else if linewise && r.end == self.buffer.len_chars() && r.start > 0 {
                    r.start -= 1;
                }
                self.buffer.remove(r.clone());
                self.buffer.cursor = r.start.min(self.buffer.len_chars());
                self.buffer.anchor = None;
                if linewise && op == 'd' {
                    let l = self.buffer.cursor_line_col().0;
                    self.buffer.cursor = self.buffer.first_non_blank(l);
                }
                self.buffer.break_group();
            }
            _ => {}
        }
        self.leave_visual();
        if op == 'c' {
            self.mode = Mode::Insert;
        } else {
            self.clamp_normal();
        }
        self.buffer.goal_col = self.buffer.cursor_line_col().1;
    }

    fn put_register(&mut self, before: bool) {
        let (text, linewise) = match self.register.clone() {
            Some(r) => r,
            None => match self.clip.get() {
                Ok(t) if !t.is_empty() => (t, false),
                _ => return self.flash("nothing to paste"),
            },
        };
        self.buffer.break_group();
        let b = &mut self.buffer;
        let (l, _) = b.cursor_line_col();
        if linewise {
            if before {
                let at = b.line_start(l);
                b.insert(at, &text);
                b.cursor = b.first_non_blank(l);
            } else {
                let r = b.line_range(l);
                if r.end == b.len_chars() && !b.slice(r.clone()).ends_with('\n') {
                    b.insert(r.end, "\n");
                    b.insert(r.end + 1, text.trim_end_matches('\n'));
                } else {
                    b.insert(r.end, &text);
                }
                b.cursor = b.first_non_blank(l + 1);
            }
        } else {
            let at = if before || b.line_len(l) == 0 { b.cursor } else { b.cursor + 1 };
            b.insert(at, &text);
            b.cursor = at + text.chars().count().saturating_sub(1);
        }
        self.buffer.break_group();
        self.clamp_normal();
    }

    fn normal_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if k.code == KeyCode::Esc {
            self.reset_pending();
            self.buffer.anchor = None;
            if self.find.open {
                self.find.open = false;
            }
            return;
        }
        // operate on a mouse-made selection
        if let (Some(range), KeyCode::Char(c @ ('d' | 'x' | 'y' | 'c')), false) = (self.buffer.selection(), k.code, ctrl) {
            let op = if c == 'x' { 'd' } else { c };
            self.apply_op(op, range, false);
            return;
        }
        if let KeyCode::Char(c) = k.code
            && !ctrl
        {
            // counts
            if c.is_ascii_digit() && (c != '0' || self.count.is_some()) {
                self.count = Some(self.count.unwrap_or(0) * 10 + c.to_digit(10).unwrap() as usize);
                return;
            }
            if self.g_pending {
                self.g_pending = false;
                if c == 'g' {
                    let cnt = self.take_count();
                    let t = self.resolve(Motion::FileStart, cnt);
                    return self.finish_motion(t);
                }
                return self.reset_pending();
            }
            if c == 'g' {
                self.g_pending = true;
                return;
            }
            if matches!(c, 'd' | 'y' | 'c') {
                if self.op == Some(c) {
                    // dd / yy / cc
                    let n = self.take_count().unwrap_or(1);
                    let l = self.buffer.cursor_line_col().0;
                    let last = (l + n - 1).min(self.buffer.line_count() - 1);
                    let r = self.buffer.line_range(l).start..self.buffer.line_range(last).end;
                    let op = self.op.take().unwrap();
                    return self.apply_op(op, r, true);
                }
                self.op = Some(c);
                return;
            }
        }
        // `gg` with operator pending: handled via g_pending path above (FileStart)
        if !ctrl {
            if let Some(m) = self.motion_for(&k) {
                let cnt = self.take_count();
                // vi quirk: `cw` changes to the end of the word, like `ce`
                let t = if self.op == Some('c') && m == Motion::WordFwd { self.change_word_target(cnt) } else { self.resolve(m, cnt) };
                return self.finish_motion(t);
            }
        } else if let KeyCode::Char(c) = k.code {
            match c {
                'r' => return self.redo(),
                'd' | 'u' => {
                    let half = (self.layout(self.area).text.height / 2).max(1) as usize;
                    let (l, _) = self.buffer.cursor_line_col();
                    let nl = if c == 'd' { l + half } else { l.saturating_sub(half) };
                    let p = self.buffer.pos_of(nl, self.buffer.goal_col);
                    return self.move_to(p, false, true);
                }
                _ => {}
            }
            return;
        }
        if self.nav_key(k) {
            self.reset_pending();
            return;
        }
        let KeyCode::Char(c) = k.code else {
            return self.reset_pending();
        };
        let cnt = self.take_count();
        if self.op.is_some() {
            return self.reset_pending(); // unknown motion after operator
        }
        let b = &self.buffer;
        let (line, _) = b.cursor_line_col();
        match c {
            'i' => self.set_mode(Mode::Insert),
            'a' => {
                if b.line_len(line) > 0 {
                    self.buffer.cursor += 1;
                }
                self.set_mode(Mode::Insert)
            }
            'I' => {
                self.buffer.cursor = b.first_non_blank(line);
                self.set_mode(Mode::Insert)
            }
            'A' => {
                self.buffer.cursor = b.line_end(line);
                self.set_mode(Mode::Insert)
            }
            'o' | 'O' => {
                let indent: String = b.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                self.buffer.break_group();
                if c == 'o' {
                    let e = self.buffer.line_end(line);
                    self.buffer.insert(e, &format!("\n{indent}"));
                    self.buffer.cursor = e + 1 + indent.chars().count();
                } else {
                    let s = self.buffer.line_start(line);
                    self.buffer.insert(s, &format!("{indent}\n"));
                    self.buffer.cursor = s + indent.chars().count();
                }
                self.set_mode(Mode::Insert);
            }
            'x' | 'X' => {
                let n = cnt.unwrap_or(1);
                let (s, e) = if c == 'x' {
                    (b.cursor, (b.cursor + n).min(b.line_end(line)))
                } else {
                    (b.cursor.saturating_sub(n).max(b.line_start(line)), b.cursor)
                };
                if s < e {
                    self.apply_op('d', s..e, false);
                }
            }
            'D' | 'C' => {
                let e = b.line_end(line);
                let s = b.cursor;
                self.apply_op(if c == 'D' { 'd' } else { 'c' }, s..e, false);
            }
            'Y' => {
                let r = b.line_range(line);
                self.apply_op('y', r, true);
            }
            'p' => self.put_register(false),
            'P' => self.put_register(true),
            'u' => self.undo(),
            'v' => {
                self.buffer.anchor = Some(self.buffer.cursor);
                self.set_mode(Mode::Visual);
            }
            'V' => {
                self.buffer.anchor = Some(self.buffer.cursor);
                self.set_mode(Mode::VisualLine);
            }
            ':' => self.open_prompt(""),
            '/' => self.open_find(false),
            'n' => self.find_step(true),
            'N' => self.find_step(false),
            _ => {}
        }
    }

    fn finish_motion(&mut self, t: Target) {
        if let Some(op) = self.op.take() {
            let (r, linewise) = self.op_range(&t);
            self.count = None;
            return self.apply_op(op, r, linewise);
        }
        let vertical = t.linewise;
        self.move_to(t.pos, false, vertical);
    }

    fn visual_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if k.code == KeyCode::Esc {
            self.buffer.anchor = None;
            self.mode = Mode::Normal;
            self.reset_pending();
            return self.clamp_normal();
        }
        if let KeyCode::Char(c) = k.code
            && !ctrl
        {
            if c.is_ascii_digit() && (c != '0' || self.count.is_some()) {
                self.count = Some(self.count.unwrap_or(0) * 10 + c.to_digit(10).unwrap() as usize);
                return;
            }
            if self.g_pending {
                self.g_pending = false;
                if c == 'g' {
                    let cnt = self.take_count();
                    let t = self.resolve(Motion::FileStart, cnt);
                    return self.move_to(t.pos, false, true);
                }
                return;
            }
            let linewise = self.mode == Mode::VisualLine;
            match c {
                'g' => return self.g_pending = true,
                'v' => {
                    self.mode = if self.mode == Mode::Visual {
                        self.buffer.anchor = None;
                        Mode::Normal
                    } else {
                        Mode::Visual
                    };
                    return self.clamp_normal();
                }
                'V' => {
                    self.mode = if linewise {
                        self.buffer.anchor = None;
                        Mode::Normal
                    } else {
                        Mode::VisualLine
                    };
                    return self.clamp_normal();
                }
                'o' => {
                    let a = self.buffer.anchor.unwrap_or(self.buffer.cursor);
                    self.buffer.anchor = Some(self.buffer.cursor);
                    self.buffer.cursor = a;
                    return;
                }
                'd' | 'x' | 'y' | 'c' => {
                    if let Some(r) = self.selection_range() {
                        let op = if c == 'x' { 'd' } else { c };
                        self.mode = Mode::Normal;
                        self.apply_op(op, r, linewise);
                    }
                    return;
                }
                'p' => {
                    if let Some(r) = self.selection_range() {
                        self.mode = Mode::Normal;
                        self.buffer.anchor = None;
                        let reg = self.register.clone();
                        self.buffer.break_group();
                        self.buffer.remove(r.clone());
                        self.buffer.cursor = r.start;
                        if let Some((t, lw)) = reg {
                            self.buffer.insert_at_cursor(&t);
                            if lw {
                                self.buffer.cursor = r.start;
                            }
                        }
                        self.buffer.break_group();
                        self.clamp_normal();
                    }
                    return;
                }
                ':' => return self.open_prompt(""),
                '/' => return self.open_find(false),
                _ => {}
            }
            if let Some(m) = self.motion_for(&k) {
                let cnt = self.take_count();
                let t = self.resolve(m, cnt);
                return self.move_to(t.pos, false, t.linewise);
            }
        }
        self.nav_key(k);
    }

    // ------------------------------------------------------------------
    // find bar
    // ------------------------------------------------------------------

    pub fn open_find(&mut self, replace: bool) {
        self.find.open = true;
        self.find.replace_mode = replace;
        self.find.in_replace = false;
        self.focus = Focus::Find;
        if let Some(t) = self.selection_range().map(|r| self.buffer.slice(r)).filter(|t| !t.contains('\n') && !t.is_empty()) {
            self.find.query.set(&t);
        }
        self.find.query.end();
        self.find.refresh(&self.buffer);
    }

    fn close_find(&mut self) {
        self.find.open = false;
        self.focus = Focus::Editor;
    }

    fn select_match(&mut self, i: usize) {
        if let Some(m) = self.find.matches.get(i).cloned() {
            self.buffer.break_group();
            self.buffer.anchor = Some(m.start);
            self.buffer.cursor = m.end;
            if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                self.mode = Mode::Normal;
            }
            self.follow = true;
        }
    }

    fn find_changed(&mut self) {
        self.find.refresh(&self.buffer);
        let from = self.buffer.selection().map(|r| r.start).unwrap_or(self.buffer.cursor);
        if let Some(i) = self.find.incremental_target(from) {
            self.select_match(i);
        } else {
            self.buffer.anchor = None;
        }
    }

    /// Next / previous match relative to the current selection (or the cursor when nothing is selected).
    pub fn find_step(&mut self, forward: bool) {
        if self.find.query.is_empty() {
            return self.flash("no search pattern (Ctrl+F)");
        }
        // matches are only kept fresh while the bar is open; compute them either way
        let was_open = std::mem::replace(&mut self.find.open, true);
        self.find.refresh(&self.buffer);
        self.find.open = was_open;
        let m = &self.find.matches;
        let idx = match (self.buffer.selection(), forward) {
            (Some(sel), true) => next_match(m, sel.start),
            (Some(sel), false) => prev_match(m, sel.start),
            (None, true) => self.find.incremental_target(self.buffer.cursor),
            (None, false) => prev_match(m, self.buffer.cursor),
        };
        match idx {
            Some(i) => self.select_match(i),
            None => self.flash("no matches"),
        }
    }

    fn replace_current(&mut self) {
        self.find.refresh(&self.buffer);
        let text = self.buffer.text();
        let idx = match self.find.current_index(&self.buffer) {
            Some(i) => i,
            None => {
                // nothing selected yet: select the next match first (VS Code behaviour)
                let from = self.buffer.cursor;
                match self.find.incremental_target(from) {
                    Some(i) => return self.select_match(i),
                    None => return self.flash("no matches"),
                }
            }
        };
        let edits = match self.find.finder().replacements(&text, &self.find.repl.text) {
            Ok(e) => e,
            Err(e) => return self.flash(e.to_string()),
        };
        if let Some((r, s)) = edits.get(idx).cloned() {
            self.buffer.break_group();
            self.buffer.replace(r.clone(), &s);
            self.buffer.break_group();
            self.buffer.cursor = r.start + s.chars().count();
            self.buffer.anchor = None;
            self.find.refresh(&self.buffer);
            if let Some(i) = self.find.incremental_target(self.buffer.cursor) {
                self.select_match(i);
            }
        }
    }

    fn replace_all(&mut self) {
        let text = self.buffer.text();
        match self.find.finder().replace_all(&text, &self.find.repl.text) {
            Ok((new, 0)) => {
                let _ = new;
                self.flash("no matches")
            }
            Ok((new, n)) => {
                self.buffer.set_text_undoable(&new);
                self.find.refresh(&self.buffer);
                self.flash(format!("replaced {n} occurrence{}", if n == 1 { "" } else { "s" }));
            }
            Err(e) => self.flash(e.to_string()),
        }
    }

    fn find_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        if alt || ctrl {
            if let KeyCode::Char(c) = k.code {
                match c.to_ascii_lowercase() {
                    'c' if alt => return self.toggle_opt('c'),
                    'w' if alt => return self.toggle_opt('w'),
                    'r' if alt => return self.toggle_opt('r'),
                    'a' if alt => return self.replace_all(),
                    'u' if ctrl => {
                        if self.find.in_replace {
                            self.find.repl.kill_to_start();
                        } else {
                            self.find.query.kill_to_start();
                            self.find_changed();
                        }
                        return;
                    }
                    _ => {}
                }
            }
            if k.code == KeyCode::Enter && alt {
                return self.replace_all();
            }
        }
        let in_repl = self.find.in_replace;
        match k.code {
            KeyCode::Esc => self.close_find(),
            KeyCode::Enter if in_repl => self.replace_current(),
            KeyCode::Enter => self.find_step(!shift),
            KeyCode::Tab | KeyCode::BackTab if self.find.replace_mode => self.find.in_replace = !in_repl,
            KeyCode::Down if self.find.replace_mode && !in_repl => self.find.in_replace = true,
            KeyCode::Up if in_repl => self.find.in_replace = false,
            KeyCode::Down => self.find_step(true),
            KeyCode::Up => self.find_step(false),
            code => {
                let f = if in_repl { &mut self.find.repl } else { &mut self.find.query };
                let mut changed = false;
                match code {
                    KeyCode::Backspace => {
                        f.backspace();
                        changed = true;
                    }
                    KeyCode::Delete => {
                        f.delete();
                        changed = true;
                    }
                    KeyCode::Left => f.left(),
                    KeyCode::Right => f.right(),
                    KeyCode::Home => f.home(),
                    KeyCode::End => f.end(),
                    KeyCode::Char(c) if !ctrl && !alt => {
                        f.insert_str(&c.to_string());
                        changed = true;
                    }
                    _ => {}
                }
                if changed && !in_repl {
                    self.find_changed();
                }
            }
        }
    }

    fn toggle_opt(&mut self, which: char) {
        match which {
            'c' => self.find.case_sensitive = !self.find.case_sensitive,
            'w' => self.find.whole_word = !self.find.whole_word,
            _ => self.find.regex = !self.find.regex,
        }
        self.find_changed();
    }

    // ------------------------------------------------------------------
    // commands
    // ------------------------------------------------------------------

    pub fn exec_command(&mut self, line: &str) {
        let line = line.trim().trim_start_matches(':').trim();
        let (cmd, arg) = line.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).unwrap_or((line, ""));
        match cmd {
            "" => {}
            "w" | "write" => {
                if arg.is_empty() {
                    self.save()
                } else {
                    match self.buffer.save_as(&PathBuf::from(arg)) {
                        Ok(n) => {
                            self.update_lang();
                            self.flash(format!("\"{arg}\" {n}L written"))
                        }
                        Err(e) => self.flash(format!("cannot write {arg}: {e}")),
                    }
                }
            }
            "q" | "quit" => {
                if self.buffer.is_modified() {
                    self.flash("No write since last change (use :q! to discard, or Ctrl+Q to be asked)");
                } else {
                    self.quit = true;
                }
            }
            "q!" | "quit!" => self.quit = true,
            "wq" | "x" => {
                self.save();
                if !self.buffer.is_modified() {
                    self.quit = true;
                }
            }
            "e" | "edit" => {
                if arg.is_empty() {
                    return self.flash("usage: :e FILE");
                }
                if self.buffer.is_modified() {
                    return self.flash("No write since last change");
                }
                self.open_file(&PathBuf::from(arg));
            }
            "warnings" => {
                let m = if self.warnings.is_empty() { "no config warnings".to_string() } else { self.warnings.join(" | ") };
                self.flash(m)
            }
            "setlang" | "setlanguage" => self.exec_setlang(arg),
            "snippets" | "snippet" => self.open_snippets(),
            "connect" => self.exec_sql(&format!("connect {arg}")),
            "disconnect" => self.sql_disconnect(),
            "run" => self.sql_run_smart(),
            "runline" => self.sql_run_line(),
            "runbuffer" => self.sql_run_buffer(),
            "cancel" => self.sql_cancel(),
            "sql" => self.exec_sql(arg),
            n if n.parse::<usize>().is_ok() => {
                let l = n.parse::<usize>().unwrap().saturating_sub(1);
                let p = self.buffer.first_non_blank(l.min(self.buffer.line_count() - 1));
                self.move_to(p, false, false);
            }
            other => self.flash(format!("Not an editor command: {other}")),
        }
    }

    pub fn open_file(&mut self, path: &std::path::Path) {
        match Buffer::load(path) {
            Ok(b) => self.buffer = b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.buffer = Buffer::new();
                self.buffer.path = Some(path.to_path_buf());
                self.flash(format!("\"{}\" [New File]", path.display()));
            }
            Err(e) => return self.flash(format!("cannot open {}: {e}", path.display())),
        }
        self.update_lang();
        self.top_line = 0;
        self.left_col = 0;
        self.syn = None;
    }

    // ------------------------------------------------------------------
    // SQL
    // ------------------------------------------------------------------

    fn exec_sql(&mut self, arg: &str) {
        let (sub, rest) = arg.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())).unwrap_or((arg, ""));
        match sub {
            "connect" => {
                if rest.is_empty() {
                    return self.flash("usage: :sql connect PROFILE");
                }
                self.sql_connect(rest)
            }
            "disconnect" => self.sql_disconnect(),
            "run" => self.sql_run_smart(),
            "runline" => self.sql_run_line(),
            "buffer" => self.sql_run_buffer(),
            "cancel" => self.sql_cancel(),
            "close" => {
                self.sql.pane_open = false;
                self.sql.tab_active = false;
                if self.focus == Focus::Table {
                    self.focus = Focus::Editor;
                }
            }
            "show" => {
                self.sql.pane_open = self.sql.result.is_some() || self.sql.session.is_some();
                if self.cfg.sql.result_view == "tab" && self.sql.result.is_some() {
                    self.sql.tab_active = true;
                    self.focus = Focus::Table;
                }
            }
            "warnings" => {
                let w = self.sql.result.as_ref().and_then(|r| r.outcome.as_ref().ok()).map(|rs| rs.warnings.clone()).unwrap_or_default();
                if w.is_empty() {
                    self.flash("no warnings on the last result");
                } else {
                    self.flash(w.join(" | "));
                }
            }
            "profiles" => match profile::load_profiles(&self.cfg.sql.connections_path()) {
                Ok(p) => self.flash(format!("profiles: {}", p.keys().cloned().collect::<Vec<_>>().join(", "))),
                Err(e) => self.flash(e),
            },
            _ => self
                .flash("usage: :connect NAME | :disconnect | :run | :runline | :runbuffer | :cancel | :sql close|show|profiles|warnings"),
        }
    }

    fn sql_connect(&mut self, name: &str) {
        let profiles = match profile::load_profiles(&self.cfg.sql.connections_path()) {
            Ok(p) => p,
            Err(e) => return self.flash(e),
        };
        match profiles.get(name) {
            None => self.flash(format!("no profile {name:?}; have: {}", profiles.keys().cloned().collect::<Vec<_>>().join(", "))),
            Some(p) if p.needs_password_prompt() => {
                self.prompt = Some(Prompt { kind: PromptKind::Password(name.to_string()), field: TextField::default() });
                self.focus = Focus::Prompt;
            }
            Some(_) => self.sql_connect_with(name, None),
        }
    }

    fn sql_connect_with(&mut self, name: &str, password: Option<String>) {
        let profiles = match profile::load_profiles(&self.cfg.sql.connections_path()) {
            Ok(p) => p,
            Err(e) => return self.flash(e),
        };
        let Some(p) = profiles.get(name) else { return self.flash(format!("no profile {name:?}")) };
        let resolved = match p.resolve(&|k| std::env::var(k).ok(), password) {
            Ok(r) => r,
            Err(e) => return self.flash(format!("profile {name}: {e}")),
        };
        self.sql_disconnect();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(sql::connect(&resolved).map(Session::new));
        });
        self.sql.connecting = Some((name.to_string(), rx));
        self.flash(format!("connecting to {name}…"));
    }

    fn sql_disconnect(&mut self) {
        if let Some(s) = &self.sql.session
            && s.is_running()
        {
            let _ = s.cancel();
        }
        if self.sql.session.take().is_some() {
            self.flash("disconnected");
        }
        self.sql.connecting = None;
    }

    fn sql_start(&mut self, text: String, origin: (usize, usize)) {
        let limit = self.cfg.sql.row_limit;
        let Some(s) = &mut self.sql.session else {
            return self.flash(DISCONNECTED_MSG);
        };
        if text.trim().is_empty() {
            return self.flash("nothing to run");
        }
        match s.run(&text, limit) {
            Ok(()) => {
                self.sql.origin = origin;
                self.sql.result = None;
                self.sql.pane_open = true;
                self.sql.tab_active = false;
            }
            Err(e) => self.flash(e),
        }
    }

    /// SQL dialect of the connected driver, for splitting statements.
    fn sql_dialect(&self) -> stmt::Dialect {
        match self.sql.session.as_ref().map(|s| s.engine) {
            Some(profile::Engine::MariaDb) => stmt::MARIADB,
            Some(profile::Engine::Postgres) => stmt::POSTGRES,
            None => stmt::GENERIC,
        }
    }

    /// Ctrl+Enter / `:run`: the selection, or the statement under the cursor when nothing is selected.
    pub fn sql_run_smart(&mut self) {
        let range = match self.selection_range() {
            Some(r) if r.start < r.end => Some(r),
            _ => stmt::statement_at(&self.buffer.text(), self.buffer.cursor, self.sql_dialect()),
        };
        let Some(r) = range else { return self.flash("nothing to run here") };
        let text = self.buffer.slice(r.clone());
        let (l, c) = self.buffer.line_col(r.start);
        if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            self.buffer.anchor = None;
            self.mode = Mode::Normal;
        }
        self.sql_start(text, (l, c));
    }

    /// `:runline`: only the line the cursor is on.
    pub fn sql_run_line(&mut self) {
        let l = self.buffer.cursor_line_col().0;
        let text = self.buffer.line(l);
        if text.trim().is_empty() {
            return self.flash("this line is empty");
        }
        self.sql_start(text, (l, 0));
    }

    pub fn sql_run_buffer(&mut self) {
        let t = self.buffer.text();
        self.sql_start(t, (0, 0));
    }

    pub fn sql_cancel(&mut self) {
        match &self.sql.session {
            Some(s) => match s.cancel() {
                Ok(()) => self.flash("cancel requested"),
                Err(e) => self.flash(e),
            },
            None => self.flash("not connected"),
        }
    }

    /// Poll background work (connect results, finished queries). Call regularly.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        if let Some((name, rx)) = &self.sql.connecting {
            match rx.try_recv() {
                Ok(Ok(sess)) => {
                    let label = sess.label.clone();
                    let name = name.clone();
                    self.sql.session = Some(sess);
                    self.sql.connecting = None;
                    self.flash(format!("connected: {name} ({label})"));
                    changed = true;
                }
                Ok(Err(e)) => {
                    self.sql.connecting = None;
                    let status = format!("✖ could not connect · {}", e.category.label());
                    self.sql.result = Some(ResultView::new(Err(e), Duration::ZERO, status));
                    self.sql.pane_open = true;
                    if self.cfg.sql.result_view == "tab" {
                        self.sql.tab_active = true;
                        self.focus = Focus::Table;
                    }
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(_) => {
                    self.sql.connecting = None;
                    changed = true;
                }
            }
        }
        if let Some(s) = &mut self.sql.session
            && let Some(mut rep) = s.poll()
        {
            if let Err(e) = &mut rep.result {
                e.shift(self.sql.origin.0, self.sql.origin.1);
            }
            let status = sql::status_text(&rep);
            self.sql.result = Some(ResultView::new(rep.result, rep.elapsed, status));
            self.sql.pane_open = true;
            if self.cfg.sql.result_view == "tab" {
                self.sql.tab_active = true;
                self.focus = Focus::Table;
            }
            changed = true;
        }
        changed
    }

    /// True while something animates (running query, pending connection): keep redrawing.
    pub fn is_busy(&self) -> bool {
        self.sql.connecting.is_some() || self.sql.session.as_ref().is_some_and(|s| s.is_running())
    }

    // ------------------------------------------------------------------
    // language mode and snippets (shared picker popup)
    // ------------------------------------------------------------------

    fn exec_setlang(&mut self, arg: &str) {
        if arg.is_empty() {
            return self.open_lang_picker();
        }
        if arg.eq_ignore_ascii_case("auto") {
            self.set_lang(None);
            return self.flash(format!("language: auto-detected ({})", self.lang.name));
        }
        match syntax::by_name(arg) {
            Some(l) => {
                self.set_lang(Some(l));
                self.flash(format!("language: {}", l.name));
            }
            None => {
                let names: Vec<&str> = syntax::all_modes().iter().map(|l| l.name).collect();
                self.flash(format!("unknown language {arg:?}; use one of: auto, {}", names.join(", ")));
            }
        }
    }

    fn open_lang_picker(&mut self) {
        let mut items = vec![PickerItem { label: "Auto-detect".into(), detail: "by file extension".into(), value: "auto".into() }];
        for l in syntax::all_modes() {
            let exts = if l.extensions.is_empty() {
                "no highlighting".to_string()
            } else {
                l.extensions.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(" ")
            };
            let current = if self.lang_override.is_some_and(|o| o.name == l.name) { "  (current)" } else { "" };
            items.push(PickerItem { label: l.name.to_string(), detail: format!("{exts}{current}"), value: l.name.to_string() });
        }
        let note = self.lang_override.map(|l| format!("manual mode: {} (Auto-detect to undo)", l.name));
        let mut p = Picker::new(PickerKind::Language, "Language mode", note, items);
        // start on the active choice
        p.selected = self.lang_override.and_then(|o| p.items.iter().position(|i| i.value == o.name)).unwrap_or(0);
        self.picker = Some(p);
        self.focus = Focus::Picker;
    }

    pub fn open_snippets(&mut self) {
        let engine = self.sql.session.as_ref().map(|s| s.engine);
        let (set, generic) = crate::snippets::for_engine(engine);
        let items = set
            .iter()
            .map(|s| PickerItem { label: s.name.to_string(), detail: s.description.to_string(), value: s.body.to_string() })
            .collect();
        let (title, note) = match (engine, generic) {
            (Some(crate::sql::profile::Engine::MariaDb), _) => ("SQL snippets: MariaDB", None),
            (Some(crate::sql::profile::Engine::Postgres), _) => ("SQL snippets: PostgreSQL", None),
            (None, _) => {
                ("SQL snippets: generic", Some("Not connected: showing generic snippets. Connect for driver-specific ones.".to_string()))
            }
        };
        self.picker = Some(Picker::new(PickerKind::Snippet, title, note, items));
        self.focus = Focus::Picker;
    }

    fn picker_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let Some(p) = &mut self.picker else {
            self.focus = Focus::Editor;
            return;
        };
        match k.code {
            KeyCode::Esc => {
                self.picker = None;
                self.focus = Focus::Editor;
            }
            KeyCode::Enter | KeyCode::Tab => self.picker_accept(),
            KeyCode::Up => p.move_by(-1),
            KeyCode::Down => p.move_by(1),
            KeyCode::PageUp => p.move_by(-10),
            KeyCode::PageDown => p.move_by(10),
            KeyCode::Char('p') if ctrl => p.move_by(-1),
            KeyCode::Char('n') if ctrl => p.move_by(1),
            KeyCode::Backspace => {
                p.filter.backspace();
                p.filter_changed();
            }
            KeyCode::Char(c) if !ctrl => {
                p.filter.insert_str(&c.to_string());
                p.filter_changed();
            }
            _ => {}
        }
    }

    fn picker_accept(&mut self) {
        let Some(p) = self.picker.take() else { return };
        self.focus = Focus::Editor;
        let Some(item) = p.current().cloned() else { return };
        match p.kind {
            PickerKind::Language => self.exec_setlang(&item.value),
            PickerKind::Snippet => {
                self.buffer.read_only = false; // an explicit command: allowed even while disconnected
                self.buffer.break_group();
                self.insert_text(&item.value);
                self.buffer.break_group();
                self.flash(format!("inserted snippet: {}", item.label));
            }
        }
    }

    fn picker_mouse(&mut self, m: MouseEvent) {
        let Some(p) = &mut self.picker else { return };
        let area = self.area;
        let vis = p.visible();
        let rect = picker::picker_rect(area, vis.len().max(1));
        match m.kind {
            MouseEventKind::ScrollDown => p.move_by(1),
            MouseEventKind::ScrollUp => p.move_by(-1),
            MouseEventKind::Down(MouseButton::Left) => {
                if !contains(rect, m.column, m.row) {
                    self.picker = None;
                    self.focus = Focus::Editor;
                    return;
                }
                let rows = picker::visible_rows(rect);
                let start = picker::window_start(p.selected, rows);
                for r in 0..rows.min(vis.len().saturating_sub(start)) {
                    if contains(picker::row_rect(rect, r), m.column, m.row) {
                        p.selected = start + r;
                        return self.picker_accept();
                    }
                }
            }
            _ => {}
        }
    }

    // ---- result table interaction ------------------------------------------

    fn toggle_table_focus(&mut self) {
        let tab_mode = self.cfg.sql.result_view == "tab";
        if self.focus == Focus::Table {
            self.focus = Focus::Editor;
            self.sql.tab_active = false;
        } else if self.sql.pane_open && self.sql.result.is_some() {
            self.focus = Focus::Table;
            self.sql.tab_active = tab_mode;
        } else {
            self.flash("no result table (run a query with F5/F6)");
        }
    }

    fn table_data(&self) -> Option<(&ResultSet, &TableState)> {
        let r = self.sql.result.as_ref()?;
        r.outcome.as_ref().ok().map(|rs| (rs, &r.state))
    }

    fn with_table<F: FnOnce(&mut TableState, &[usize], usize, usize, usize, usize)>(&mut self, f: F) {
        let Some((vw, vh)) = self.table_viewport() else { return };
        let opts = self.table_opts();
        let Some(rv) = &mut self.sql.result else { return };
        let Ok(rs) = &rv.outcome else { return };
        let widths = table::widths_from_natural(&rv.natural, &opts, &rv.state);
        let vw = table::visible_width(&opts, vw);
        let vrows = table::visible_row_count(&opts, vh);
        f(&mut rv.state, &widths, vw, vrows, rs.rows.len(), rs.columns.len());
        rv.state.clamp(rs.columns.len(), rs.rows.len(), &widths, vw, vrows);
    }

    pub fn scroll_table(&mut self, drows: isize, dcells: isize) {
        self.with_table(|st, widths, vw, _vr, _n, _c| {
            st.row_off = (st.row_off as isize + drows).max(0) as usize;
            if dcells != 0 {
                st.col_off = (st.col_off as isize + dcells).max(0) as usize;
                // keep cur_col pointing at the leftmost visible column
                let spans = table::col_spans(widths);
                if let Some(i) = spans.iter().position(|(_, e)| *e > st.col_off) {
                    st.cur_col = i;
                }
                let _ = vw;
            }
        });
    }

    fn table_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let page = self.table_viewport().map(|(_, h)| table::visible_row_count(&self.table_opts(), h)).unwrap_or(10) as isize;
        match k.code {
            KeyCode::Esc | KeyCode::Tab => {
                self.focus = Focus::Editor;
                self.sql.tab_active = false;
            }
            KeyCode::Char('q') => {
                self.sql.pane_open = false;
                self.sql.tab_active = false;
                self.focus = Focus::Editor;
            }
            KeyCode::Up | KeyCode::Char('k') => self.scroll_table(-1, 0),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_table(1, 0),
            KeyCode::PageUp => self.scroll_table(-page, 0),
            KeyCode::PageDown => self.scroll_table(page, 0),
            KeyCode::Char('g') => self.scroll_table(isize::MIN / 2, 0),
            KeyCode::Char('G') => self.scroll_table(isize::MAX / 2, 0),
            KeyCode::Home => self.with_table(|st, w, vw, _, _, _| {
                st.cur_col = 0;
                st.reveal_cur_col(w, vw);
                st.col_off = 0;
            }),
            KeyCode::End => self.with_table(|st, w, vw, _, _, n| {
                st.cur_col = n.saturating_sub(1);
                st.reveal_cur_col(w, vw);
            }),
            KeyCode::Left if ctrl || shift => self.scroll_table(0, -8),
            KeyCode::Right if ctrl || shift => self.scroll_table(0, 8),
            KeyCode::Char('H') => self.scroll_table(0, -8),
            KeyCode::Char('L') => self.scroll_table(0, 8),
            KeyCode::Left | KeyCode::Char('h') => self.with_table(|st, w, vw, _, _, _| {
                st.cur_col = st.cur_col.saturating_sub(1);
                st.reveal_cur_col(w, vw);
            }),
            KeyCode::Right | KeyCode::Char('l') => self.with_table(|st, w, vw, _, _, n| {
                st.cur_col = (st.cur_col + 1).min(n.saturating_sub(1));
                st.reveal_cur_col(w, vw);
            }),
            KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char(' ') => self.with_table(|st, w, vw, _, _, _| {
                let c = st.cur_col;
                st.toggle_expand(c);
                let _ = (w, vw);
            }),
            _ => {}
        }
        // after (un)expanding, make sure the current column is still in view
        if matches!(k.code, KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char(' ')) {
            let opts = self.table_opts();
            if let (Some((vw, _)), Some(rv)) = (self.table_viewport(), &mut self.sql.result)
                && rv.outcome.is_ok()
            {
                let widths = table::widths_from_natural(&rv.natural, &opts, &rv.state);
                rv.state.reveal_cur_col(&widths, table::visible_width(&opts, vw));
            }
        }
    }

    fn copy_table(&mut self) {
        let Some((rs, _)) = self.table_data() else { return };
        let mut out = rs.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>().join("\t");
        for r in &rs.rows {
            out.push('\n');
            out.push_str(&r.join("\t"));
        }
        let n = rs.rows.len();
        self.set_clipboard(&out);
        self.flash(format!("copied {n} rows as TSV"));
    }

    // ------------------------------------------------------------------
    // mouse
    // ------------------------------------------------------------------

    pub fn handle_mouse(&mut self, m: MouseEvent) {
        self.guarded(|a| a.handle_mouse_inner(m));
        self.refresh_suggestions();
    }

    fn handle_mouse_inner(&mut self, m: MouseEvent) {
        if !self.cfg.editor.mouse || matches!(self.focus, Focus::Prompt | Focus::Confirm) {
            return;
        }
        if self.focus == Focus::Picker {
            return self.picker_mouse(m);
        }
        let l = self.layout(self.area);
        let (x, y) = (m.column, m.row);
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);

        // result pane
        if let Some(p) = l.pane
            && contains(p, x, y)
        {
            return self.pane_mouse(m, p);
        }
        // find bar
        if self.find.open && findbar::bar_fits(l.text, self.find.replace_mode) {
            let bl = findbar::bar_layout(l.text, self.find.replace_mode);
            if contains(bl.rect, x, y) {
                return self.bar_mouse(m, &bl);
            }
        }
        if !contains(l.text, x, y) {
            return;
        }
        let tab = self.cfg.editor.tab_width;
        let in_gutter = x < l.text.x + l.gutter_w;
        let line_at = |app: &App| (app.top_line + (y - l.text.y) as usize).min(app.buffer.line_count() - 1);
        let pos_at = |app: &App| {
            let line = line_at(app);
            let cell = app.left_col + x.saturating_sub(l.text.x + l.gutter_w) as usize;
            app.buffer.pos_of(line, col_at_x(&app.buffer.line(line), cell, tab))
        };
        match m.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if shift => {
                let d = self.cfg.editor.scroll_lines * 2;
                self.left_col = if m.kind == MouseEventKind::ScrollDown { self.left_col + d } else { self.left_col.saturating_sub(d) };
                self.follow = false;
            }
            MouseEventKind::ScrollDown => self.scroll_text(self.cfg.editor.scroll_lines as isize),
            MouseEventKind::ScrollUp => self.scroll_text(-(self.cfg.editor.scroll_lines as isize)),
            MouseEventKind::ScrollRight => {
                self.left_col += self.cfg.editor.scroll_lines * 2;
                self.follow = false;
            }
            MouseEventKind::ScrollLeft => {
                self.left_col = self.left_col.saturating_sub(self.cfg.editor.scroll_lines * 2);
                self.follow = false;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.focus = Focus::Editor;
                self.follow = true;
                self.buffer.break_group();
                if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                    self.mode = Mode::Normal;
                }
                let count = self.register_click(x, y);
                if in_gutter {
                    let line = line_at(self);
                    if line >= self.buffer.line_count() {
                        return;
                    }
                    let r = self.buffer.line_range(line);
                    self.buffer.anchor = Some(r.start);
                    self.buffer.cursor = r.end;
                    self.drag_origin = None;
                    return;
                }
                let pos = pos_at(self);
                match count {
                    1 => {
                        self.buffer.anchor = None;
                        self.buffer.cursor = pos;
                        self.buffer.goal_col = self.buffer.cursor_line_col().1;
                        self.drag_origin = Some(pos);
                        self.clamp_normal();
                    }
                    2 => {
                        let r = self.buffer.word_range_at(pos);
                        self.buffer.anchor = Some(r.start);
                        self.buffer.cursor = r.end;
                        self.drag_origin = None;
                    }
                    _ => {
                        let r = self.buffer.line_range(self.buffer.line_of(pos));
                        self.buffer.anchor = Some(r.start);
                        self.buffer.cursor = r.end;
                        self.drag_origin = None;
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(o) = self.drag_origin {
                    let pos = pos_at(self);
                    self.buffer.anchor = Some(o);
                    self.buffer.cursor = pos;
                    self.follow = true;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {}
            _ => {}
        }
    }

    fn scroll_text(&mut self, d: isize) {
        let max = self.buffer.line_count().saturating_sub(1) as isize;
        self.top_line = (self.top_line as isize + d).clamp(0, max) as usize;
        self.follow = false;
    }

    fn register_click(&mut self, x: u16, y: u16) -> u8 {
        let now = (self.clock)();
        let window = Duration::from_millis(self.cfg.editor.double_click_ms);
        let count = match &self.click {
            Some(c) if now.duration_since(c.at) <= window && c.pos == (x, y) => (c.count % 3) + 1,
            _ => 1,
        };
        self.click = Some(Click { at: now, pos: (x, y), count });
        count
    }

    fn bar_mouse(&mut self, m: MouseEvent, bl: &findbar::BarLayout) {
        let (x, y) = (m.column, m.row);
        if let MouseEventKind::Down(MouseButton::Left) = m.kind {
            if contains(bl.close_btn, x, y) {
                self.close_find();
            } else if contains(bl.case_btn, x, y) {
                self.toggle_opt('c');
            } else if contains(bl.word_btn, x, y) {
                self.toggle_opt('w');
            } else if contains(bl.regex_btn, x, y) {
                self.toggle_opt('r');
            } else if contains(bl.next_btn, x, y) {
                self.find_step(true);
            } else if contains(bl.prev_btn, x, y) {
                self.find_step(false);
            } else if bl.repl_one.is_some_and(|r| contains(r, x, y)) {
                self.replace_current();
            } else if bl.repl_all.is_some_and(|r| contains(r, x, y)) {
                self.replace_all();
            } else if bl.repl_input.is_some_and(|r| contains(r, x, y)) {
                self.focus = Focus::Find;
                self.find.in_replace = true;
            } else if contains(bl.input, x, y) {
                self.focus = Focus::Find;
                self.find.in_replace = false;
            }
        }
    }

    fn pane_mouse(&mut self, m: MouseEvent, pane: Rect) {
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
        match m.kind {
            MouseEventKind::ScrollDown if shift => self.scroll_table(0, 4),
            MouseEventKind::ScrollUp if shift => self.scroll_table(0, -4),
            MouseEventKind::ScrollDown => self.scroll_table(3, 0),
            MouseEventKind::ScrollUp => self.scroll_table(-3, 0),
            MouseEventKind::ScrollRight => self.scroll_table(0, 4),
            MouseEventKind::ScrollLeft => self.scroll_table(0, -4),
            MouseEventKind::Down(MouseButton::Left) => {
                if self.sql.result.is_none() {
                    return;
                }
                self.focus = Focus::Table;
                let opts = self.table_opts();
                let double = self.register_click(m.column, m.row) >= 2;
                let rel_x = (m.column - pane.x) as usize;
                if let Some(rv) = &mut self.sql.result
                    && rv.outcome.is_ok()
                {
                    let widths = table::widths_from_natural(&rv.natural, &opts, &rv.state);
                    if let Some(c) = table::column_at(&widths, rv.state.col_off, rel_x) {
                        rv.state.cur_col = c;
                        if double && m.row < pane.y + 2 {
                            rv.state.toggle_expand(c);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn syntax_kinds(&mut self) -> &Vec<Vec<Kind>> {
        let stale = match &self.syn {
            Some(c) => c.version != self.buffer.version || c.lang != self.lang.name,
            None => true,
        };
        if stale {
            let lines: Vec<String> = (0..self.buffer.line_count()).map(|i| self.buffer.line(i)).collect();
            let kinds = if self.cfg.ui.syntax_highlight {
                syntax::highlight(self.lang, &lines)
            } else {
                lines.iter().map(|l| vec![Kind::Plain; l.chars().count()]).collect()
            };
            self.syn = Some(SynCache { version: self.buffer.version, lang: self.lang.name, kinds });
        }
        &self.syn.as_ref().unwrap().kinds
    }
}
