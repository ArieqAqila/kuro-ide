//! Drawing. Only the terminal's default colors plus ANSI palette colors and
//! text modifiers are used: no cell ever gets a background color, so the
//! terminal's own background shows through in dark and light themes.

use crate::app::{App, Focus, Layout, PromptKind, cell_x};
use crate::config::ColorsConfig;
use crate::findbar;
use crate::syntax::Kind;
use crate::table;
use ratatui::buffer::Buffer as Cells;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthChar;

/// ANSI palette slot (0-15) -> terminal colour. Never RGB or 256-colour indexes.
pub fn ansi_color(slot: i8) -> Option<Color> {
    Some(match slot {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::White,
        _ => return None,
    })
}

fn slot_style(slot: i8) -> Style {
    match ansi_color(slot) {
        Some(c) => Style::default().fg(c),
        None => Style::default(),
    }
}

pub fn kind_style(k: Kind, c: &ColorsConfig) -> Style {
    match k {
        Kind::Plain => Style::default(),
        Kind::Keyword => slot_style(c.keyword),
        Kind::Builtin => slot_style(c.builtin),
        Kind::Literal => slot_style(c.literal),
        Kind::String => slot_style(c.string),
        Kind::Number => slot_style(c.number),
        Kind::Operator => slot_style(c.operator),
        Kind::Key => slot_style(c.key),
        Kind::Comment => match ansi_color(c.comment) {
            Some(col) => Style::default().fg(col).add_modifier(Modifier::ITALIC),
            None => Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC),
        },
        Kind::Bracket(d) => {
            let depth = &c.bracket_depth;
            slot_style(depth[d as usize % depth.len()])
        }
        Kind::BracketError => slot_style(c.bracket_error).add_modifier(Modifier::REVERSED | Modifier::BOLD),
    }
}

fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

impl App {
    /// Draw the whole UI into `buf` (covering `area`); returns the terminal cursor position.
    pub fn render(&mut self, buf: &mut Cells, area: Rect) -> Option<(u16, u16)> {
        self.area = area;
        let l = self.layout(area);
        self.find.refresh(&self.buffer);

        let mut cursor = self.render_text(buf, &l);
        self.render_scrollbars(buf, &l);
        if self.find.open {
            let c = findbar::render(buf, l.text, &self.find, &self.buffer, self.focus == Focus::Find);
            if self.focus == Focus::Find {
                cursor = c;
            }
        }
        if let Some(p) = l.pane {
            self.render_pane(buf, p);
        }
        if self.focus == Focus::Editor {
            self.render_suggestions(buf, &l);
        }
        self.render_status(buf, l.status);
        let pc = self.render_cmd(buf, l.cmd);
        let picker_cursor = match &self.picker {
            Some(p) if self.focus == Focus::Picker => crate::picker::render(buf, area, p),
            _ => None,
        };
        match self.focus {
            Focus::Picker => picker_cursor,
            Focus::Prompt => pc,
            Focus::Table | Focus::Confirm => None,
            Focus::Find => cursor,
            Focus::Editor => cursor,
        }
    }

    /// The vertical and horizontal scrollbars: arrows at the ends, a dim track, and a thumb in reverse video
    /// (no background colour is ever set).
    fn render_scrollbars(&self, buf: &mut Cells, l: &Layout) {
        if let Some(r) = l.vbar
            && let Some(bar) = crate::scrollbar::bar(r.height as usize, self.buffer.line_count(), l.text.height as usize, self.top_line)
        {
            draw_bar(buf, r, &bar, true);
        }
        if let Some(r) = l.hbar
            && let Some(bar) = crate::scrollbar::bar(r.width as usize, self.widest_line(), r.width as usize, self.left_col)
        {
            draw_bar(buf, r, &bar, false);
        }
    }

    fn render_text(&mut self, buf: &mut Cells, l: &Layout) -> Option<(u16, u16)> {
        let t = l.text;
        if t.width == 0 || t.height == 0 {
            return None;
        }
        let tab = self.cfg.editor.tab_width;
        let h = t.height as usize;
        let avail = t.width.saturating_sub(l.gutter_w) as usize;
        let (cl, cc) = self.buffer.cursor_line_col();
        let lc = self.buffer.line_count();

        if self.follow {
            let margin = if h > 6 { 1 } else { 0 };
            if cl < self.top_line + margin {
                self.top_line = cl.saturating_sub(margin);
            } else if cl + margin >= self.top_line + h {
                self.top_line = cl + margin + 1 - h;
            }
            let cx = cell_x(&self.buffer.line(cl), cc, tab);
            if avail > 0 {
                if cx < self.left_col {
                    self.left_col = cx;
                } else if cx >= self.left_col + avail {
                    self.left_col = cx + 1 - avail;
                }
            }
        }
        self.top_line = self.top_line.min(lc - 1);
        let (top, left) = (self.top_line, self.left_col);

        let sel = self.selection_range();
        let relative = self.cfg.editor.relative_numbers;
        let nw = l.gutter_w.saturating_sub(2) as usize;
        let n_matches = if self.find.open { self.find.matches.clone() } else { vec![] };
        let kinds = self.syntax_kinds().clone();
        let colors = self.cfg.colors.clone();
        let len = self.buffer.len_chars();

        for row in 0..h {
            let line = top + row;
            if line >= lc {
                break;
            }
            let y = t.y + row as u16;
            if l.gutter_w > 0 {
                let n = if relative && line != cl { line.abs_diff(cl) } else { line + 1 };
                let s = format!("{n:>nw$}");
                let style = if line == cl { bold() } else { dim() };
                buf.set_stringn(t.x + 1, y, &s, nw, style);
            }
            let text_x = t.x + l.gutter_w;
            let text = self.buffer.line(line);
            let ls = self.buffer.line_start(line);
            let le = ls + text.chars().count();
            let first_match = n_matches.partition_point(|m| m.end <= ls);
            let line_kinds = kinds.get(line);
            let mut x = 0usize;
            for (ci, ch) in text.chars().enumerate() {
                let w = if ch == '\t' {
                    tab - x % tab
                } else if ch.is_control() {
                    1
                } else {
                    UnicodeWidthChar::width(ch).unwrap_or(0)
                };
                if w == 0 {
                    continue;
                }
                if x < left {
                    x += w;
                    continue;
                }
                if x + w > left + avail {
                    break;
                }
                let abs = ls + ci;
                let mut style = line_kinds.and_then(|k| k.get(ci)).map(|k| kind_style(*k, &colors)).unwrap_or_default();
                if n_matches[first_match..].iter().take_while(|m| m.start < le).any(|m| m.start <= abs && abs < m.end) {
                    style = style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
                }
                if sel.as_ref().is_some_and(|r| r.contains(&abs)) {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                let s: String = if ch == '\t' {
                    " ".repeat(w)
                } else if ch.is_control() {
                    "·".into()
                } else {
                    ch.to_string()
                };
                buf.set_string(text_x + (x - left) as u16, y, &s, style);
                x += w;
            }
            // selected newline: show a reversed blank so the selection reads as covering the break
            if sel.as_ref().is_some_and(|r| r.contains(&le)) && le < len && x >= left && x < left + avail {
                buf.set_string(text_x + (x - left) as u16, y, " ", Style::default().add_modifier(Modifier::REVERSED));
            }
        }

        let cx = cell_x(&self.buffer.line(cl), cc, tab);
        if cl >= top && cl < top + h && cx >= left && cx < left + avail.max(1) {
            Some((t.x + l.gutter_w + (cx - left) as u16, t.y + (cl - top) as u16))
        } else {
            None
        }
    }

    /// The word-suggestion popup, anchored under the word being typed (above it when there is no room).
    fn render_suggestions(&self, buf: &mut Cells, l: &Layout) {
        let Some(st) = &self.suggest else { return };
        let t = l.text;
        let tab = self.cfg.editor.tab_width;
        let (cl, cc) = self.buffer.cursor_line_col();
        if cl < self.top_line || cl >= self.top_line + t.height as usize {
            return;
        }
        let prefix_chars = st.sug.prefix.chars().count();
        let line = self.buffer.line(cl);
        let px = cell_x(&line, cc.saturating_sub(prefix_chars), tab).saturating_sub(self.left_col);
        let y = t.y + (cl - self.top_line) as u16;
        let widest = st.sug.items.iter().map(|w| table::display_width(w)).max().unwrap_or(1);
        let w = (widest as u16 + 4).min(40).min(t.width);
        let h = st.sug.items.len() as u16 + 2;
        let x = (t.x + l.gutter_w + px as u16).min(t.right().saturating_sub(w));
        let below = y + 1;
        let top = if below + h <= t.bottom() || y < t.y + h { below } else { y - h };
        let rect = Rect::new(x, top, w, h.min(t.bottom().saturating_sub(top)));
        if rect.height < 3 || rect.width < 6 {
            return;
        }
        use ratatui::widgets::{Block, BorderType, Clear, Widget};
        Clear.render(rect, buf);
        Block::bordered().border_type(BorderType::Rounded).render(rect, buf);
        for (i, word) in st.sug.items.iter().enumerate().take(rect.height as usize - 2) {
            let style = if i == st.selected { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
            let inner = rect.width as usize - 2;
            buf.set_stringn(rect.x + 1, rect.y + 1 + i as u16, format!(" {word:<w$}", w = inner - 1), inner, style);
        }
    }

    fn render_status(&self, buf: &mut Cells, r: Rect) {
        if r.width == 0 || r.height == 0 {
            return;
        }
        let name = self
            .buffer
            .path
            .as_ref()
            .map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()))
            .unwrap_or_else(|| "[No Name]".into());
        let modified = if self.buffer.is_modified() { " [+]" } else { "" };
        let (line, col) = self.buffer.cursor_line_col();
        let mut right = String::new();
        if let Some(s) = &self.sql.session {
            right.push_str(&format!("● {}  │  ", s.label));
        }
        if self.cfg.sql.result_view == "tab" && self.sql.result.is_some() && !self.sql.tab_active {
            right.push_str("F4 results  │  ");
        }
        if let Some(l) = self.sync_label() {
            right.push_str(&format!("{l}  │  "));
        }
        if let Some(sel) = self.selection_range() {
            right.push_str(&format!("{} selected  │  ", sel.end - sel.start));
        }
        let manual = if self.lang_override.is_some() { " (manual)" } else { "" };
        right.push_str(&format!("Ln {}, Col {}  │  {}{manual}", line + 1, col + 1, self.lang.name));
        let rw = table::display_width(&right);
        let mode = self.mode_label();
        let mut x = r.x;
        if !mode.is_empty() {
            buf.set_stringn(x, r.y, mode, r.width as usize, bold());
            x += mode.len() as u16 + 2;
        }
        let room = (r.right().saturating_sub(x) as usize).saturating_sub(rw + 2);
        let left = format!("{name}{modified}");
        buf.set_stringn(x, r.y, &left, room, Style::default());
        let rx = r.right().saturating_sub(rw as u16 + 1);
        if rx > x {
            buf.set_stringn(rx, r.y, &right, rw, dim());
        }
    }

    fn render_cmd(&self, buf: &mut Cells, r: Rect) -> Option<(u16, u16)> {
        if r.width == 0 || r.height == 0 {
            return None;
        }
        if let Some(p) = &self.prompt {
            let (prefix, masked) = match &p.kind {
                PromptKind::Command => (":".to_string(), false),
                PromptKind::Password(n) => (format!("password for {n}: "), true),
            };
            buf.set_stringn(r.x, r.y, &prefix, r.width as usize, bold());
            let fr = Rect::new(r.x + prefix.chars().count() as u16, r.y, r.width.saturating_sub(prefix.chars().count() as u16), 1);
            if fr.width == 0 {
                return None;
            }
            return Some(findbar::draw_field(buf, fr, &p.field, "", masked));
        }
        if let Some(c) = &self.confirm {
            buf.set_stringn(r.x, r.y, c, r.width as usize, bold());
            return None;
        }
        if let Some(m) = &self.msg {
            buf.set_stringn(r.x, r.y, m, r.width as usize, Style::default());
        } else if self.edit_blocked() {
            buf.set_stringn(r.x, r.y, crate::app::DISCONNECTED_MSG, r.width as usize, bold());
        } else if let Some(c) = &self.sql.connecting {
            buf.set_stringn(r.x, r.y, format!("connecting to {}…", c.name), r.width as usize, dim());
        } else if !self.warnings.is_empty() {
            buf.set_stringn(r.x, r.y, format!("{} config warning(s): :warnings", self.warnings.len()), r.width as usize, dim());
        }
        None
    }

    fn render_pane(&self, buf: &mut Cells, p: Rect) {
        if p.height < 4 {
            return;
        }
        let w = p.width as usize;
        let body_h = p.height as usize - 3;
        let status_y = p.bottom() - 1;
        let focused = self.focus == Focus::Table;
        let running = self.sql.session.as_ref().and_then(|s| s.running_since);

        let Some(rv) = &self.sql.result else {
            let msg = match running {
                Some(t0) => format!("⏳ running… {:.1}s   (F7 or :sql cancel to stop)", t0.elapsed().as_secs_f32()),
                None => "no results yet — F5 runs the selection, F6 the whole buffer".into(),
            };
            buf.set_stringn(p.x, p.y, "─".repeat(w), w, dim());
            buf.set_stringn(p.x, p.y + 1, &msg, w, bold());
            return;
        };

        match &rv.outcome {
            Ok(rs) => {
                let opts = self.table_opts();
                let r = table::layout_cached(&rs.columns, &rs.rows, &rv.natural, &opts, &rv.state, w, body_h);
                buf.set_stringn(p.x, p.y, &r.header, w, bold());
                buf.set_stringn(p.x, p.y + 1, &r.separator, w, dim());
                if rs.columns.is_empty() {
                    buf.set_stringn(p.x, p.y + 2, "(statement returned no result set)", w, dim());
                }
                for (i, line) in r.rows.iter().enumerate() {
                    buf.set_stringn(p.x, p.y + 2 + i as u16, line, w, Style::default());
                }
                // SQL NULLs are drawn dim and italic (no background): distinct from the text 'NULL'
                let widths_for_nulls = table::widths_from_natural(&rv.natural, &opts, &rv.state);
                let spans = table::col_spans(&widths_for_nulls);
                for (i, row_idx) in (r.row_off..r.row_off + r.rows.len()).enumerate() {
                    for (&(nr, nc), _) in rs.nulls.range((row_idx, 0)..=(row_idx, usize::MAX)).map(|k| (k, ())) {
                        debug_assert_eq!(nr, row_idx);
                        let Some(&(s, e)) = spans.get(nc) else { continue };
                        let (s, e) = (s.max(r.col_off), e.min(r.col_off + r.visible_w));
                        if e > s {
                            buf.set_style(
                                Rect::new(p.x + (s - r.col_off) as u16, p.y + 2 + i as u16, (e - s) as u16, 1),
                                Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC),
                            );
                        }
                    }
                }
                // current column marker (reverse video on its header cell)
                if focused {
                    let widths = table::widths_from_natural(&rv.natural, &opts, &rv.state);
                    if let Some(&(s, e)) = table::col_spans(&widths).get(rv.state.cur_col) {
                        let (s, e) = (s.max(r.col_off), e.min(r.col_off + r.visible_w));
                        if e > s {
                            buf.set_style(
                                Rect::new(p.x + (s - r.col_off) as u16, p.y, (e - s) as u16, 1),
                                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
                            );
                        }
                    }
                }
                let pos = if r.total_rows == 0 {
                    String::new()
                } else {
                    format!(
                        "rows {}-{}/{}  cols {}/{}",
                        r.row_off + 1,
                        r.row_off + r.rows.len(),
                        r.total_rows,
                        rv.state.cur_col + 1,
                        rs.columns.len()
                    )
                };
                self.draw_pane_status(buf, p, status_y, &rv.status, &pos, focused, running);
            }
            Err(e) => {
                buf.set_stringn(p.x, p.y, "─".repeat(w), w, dim());
                for (i, line) in self.error_lines(e, w).iter().take(body_h + 1).enumerate() {
                    let style = if i == 0 { bold() } else { Style::default() };
                    buf.set_stringn(p.x, p.y + 1 + i as u16, line, w, style);
                }
                self.draw_pane_status(buf, p, status_y, &rv.status, "", focused, running);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_pane_status(
        &self,
        buf: &mut Cells,
        p: Rect,
        y: u16,
        status: &str,
        pos: &str,
        focused: bool,
        running: Option<std::time::Instant>,
    ) {
        let w = p.width as usize;
        let left = match running {
            Some(t0) => format!("⏳ running… {:.1}s (F7 cancels)", t0.elapsed().as_secs_f32()),
            None => status.to_string(),
        };
        buf.set_stringn(p.x, y, &left, w, bold());
        let hint = if focused { "arrows scroll · e expand · Esc back" } else { "F4 focus" };
        let lw = table::display_width(&left);
        // the scroll position matters more than the hint: drop the hint first
        let mut right = if pos.is_empty() { hint.to_string() } else { format!("{pos}  │  {hint}") };
        if lw + table::display_width(&right) + 3 > w {
            right = pos.to_string();
        }
        let rw = table::display_width(&right);
        if rw > 0 && rw + lw + 2 <= w {
            buf.set_stringn(p.x + (w - rw) as u16, y, &right, rw, dim());
        }
    }
}

/// Test helper: the characters of a rendered buffer, one string per row.
pub fn rows_text(buf: &Cells) -> Vec<String> {
    let a = buf.area;
    (a.y..a.bottom())
        .map(|y| {
            let mut row = String::new();
            let mut skip = 0;
            for x in a.x..a.right() {
                if skip > 0 {
                    // continuation cell of a wide character
                    skip -= 1;
                    continue;
                }
                let sym = buf[(x, y)].symbol();
                skip = table::display_width(sym).saturating_sub(1);
                row.push_str(sym);
            }
            row
        })
        .collect()
}

/// Draw one scrollbar into `r` (a strip one cell thick).
fn draw_bar(buf: &mut Cells, r: Rect, bar: &crate::scrollbar::Bar, vertical: bool) {
    let thumb = bar.thumb_cells();
    for i in 0..bar.len {
        let (x, y) = if vertical { (r.x, r.y + i as u16) } else { (r.x + i as u16, r.y) };
        let (sym, style) = if bar.arrows && i == 0 {
            (if vertical { "▲" } else { "◀" }, Style::default())
        } else if bar.arrows && i + 1 == bar.len {
            (if vertical { "▼" } else { "▶" }, Style::default())
        } else if thumb.contains(&i) {
            (" ", Style::default().add_modifier(Modifier::REVERSED))
        } else {
            (if vertical { "│" } else { "─" }, dim())
        };
        buf.set_string(x, y, sym, style);
    }
}
