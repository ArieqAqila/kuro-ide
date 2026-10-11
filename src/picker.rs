//! A small filterable list popup (language picker, snippet picker). Drawn without any background:
//! cleared box + border, the selected row in reverse video.

use crate::findbar::TextField;
use ratatui::buffer::Buffer as Cells;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, BorderType, Clear, Widget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    Language,
    Snippet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerItem {
    pub label: String,
    pub detail: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct Picker {
    pub kind: PickerKind,
    pub title: String,
    /// A line shown under the list, e.g. "not connected: showing generic snippets".
    pub note: Option<String>,
    pub items: Vec<PickerItem>,
    pub filter: TextField,
    /// Index into `visible()`.
    pub selected: usize,
}

impl Picker {
    pub fn new(kind: PickerKind, title: &str, note: Option<String>, items: Vec<PickerItem>) -> Picker {
        Picker { kind, title: title.to_string(), note, items, filter: TextField::default(), selected: 0 }
    }

    /// Indexes of the items matching the filter (case-insensitive substring of label or detail).
    pub fn visible(&self) -> Vec<usize> {
        let f = self.filter.text.to_lowercase();
        (0..self.items.len())
            .filter(|&i| {
                f.is_empty() || self.items[i].label.to_lowercase().contains(&f) || self.items[i].detail.to_lowercase().contains(&f)
            })
            .collect()
    }

    pub fn current(&self) -> Option<&PickerItem> {
        self.visible().get(self.selected).map(|&i| &self.items[i])
    }

    pub fn move_by(&mut self, delta: isize) {
        let n = self.visible().len();
        if n == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
    }

    /// Call after the filter text changed.
    pub fn filter_changed(&mut self) {
        self.selected = 0;
    }
}

const MAX_ROWS: usize = 14;

/// Popup rectangle, centred in `area`.
pub fn picker_rect(area: Rect, rows: usize) -> Rect {
    let w = 70.min(area.width.saturating_sub(2));
    let h = ((rows.clamp(1, MAX_ROWS) + 4) as u16).min(area.height);
    Rect::new(area.x + (area.width.saturating_sub(w)) / 2, area.y + (area.height.saturating_sub(h)) / 3, w, h)
}

/// How many list rows fit in `popup`.
pub fn visible_rows(popup: Rect) -> usize {
    (popup.height as usize).saturating_sub(4).clamp(1, MAX_ROWS)
}

/// The row rectangle of visible item `row` (0-based within the window) inside `popup`.
pub fn row_rect(popup: Rect, row: usize) -> Rect {
    Rect::new(popup.x + 1, popup.y + 2 + row as u16, popup.width.saturating_sub(2), 1)
}

/// First visible item shown in the window, keeping the selection in view.
pub fn window_start(selected: usize, rows: usize) -> usize {
    selected.saturating_sub(rows.saturating_sub(1))
}

pub fn render(buf: &mut Cells, area: Rect, p: &Picker) -> Option<(u16, u16)> {
    let vis = p.visible();
    let rect = picker_rect(area, vis.len().max(1));
    if rect.width < 12 || rect.height < 5 {
        return None;
    }
    Clear.render(rect, buf);
    Block::bordered().border_type(BorderType::Rounded).title(format!(" {} ", p.title)).render(rect, buf);
    // filter line
    let cur = crate::findbar::draw_field(
        buf,
        Rect::new(rect.x + 2, rect.y + 1, rect.width.saturating_sub(4), 1),
        &p.filter,
        "type to filter",
        false,
    );
    let rows = visible_rows(rect);
    let start = window_start(p.selected, rows);
    for (r, &idx) in vis.iter().skip(start).take(rows).enumerate() {
        let it = &p.items[idx];
        let row = row_rect(rect, r);
        let sel = start + r == p.selected;
        let style = if sel { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
        let text = format!(" {}  {}", it.label, it.detail);
        let w = row.width as usize;
        buf.set_stringn(row.x, row.y, format!("{text:<w$}"), w, style);
    }
    if vis.is_empty() {
        buf.set_stringn(rect.x + 2, rect.y + 2, "no matches", rect.width as usize - 4, Style::default().add_modifier(Modifier::DIM));
    }
    if let Some(n) = &p.note {
        buf.set_stringn(rect.x + 2, rect.bottom() - 2, n, rect.width as usize - 4, Style::default().add_modifier(Modifier::BOLD));
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(names: &[&str]) -> Vec<PickerItem> {
        names.iter().map(|n| PickerItem { label: n.to_string(), detail: format!("d-{n}"), value: n.to_string() }).collect()
    }

    #[test]
    fn filter_is_case_insensitive_and_matches_detail() {
        let mut p = Picker::new(PickerKind::Language, "t", None, items(&["Go", "Python", "SQL"]));
        assert_eq!(p.visible(), vec![0, 1, 2]);
        p.filter.set("PY");
        assert_eq!(p.visible(), vec![1]);
        p.filter.set("d-s");
        assert_eq!(p.visible(), vec![2], "detail text matches too");
        p.filter.set("zzz");
        assert!(p.visible().is_empty() && p.current().is_none());
    }

    #[test]
    fn selection_wraps_and_resets_on_filter_change() {
        let mut p = Picker::new(PickerKind::Snippet, "t", None, items(&["a", "b", "c"]));
        p.move_by(-1);
        assert_eq!(p.current().unwrap().label, "c", "wraps to the end");
        p.move_by(1);
        assert_eq!(p.current().unwrap().label, "a");
        p.move_by(1);
        p.filter.set("b");
        p.filter_changed();
        assert_eq!(p.selected, 0);
        assert_eq!(p.current().unwrap().label, "b");
        p.filter.set("none");
        p.move_by(1);
        assert_eq!(p.selected, 0, "empty list: no panic");
    }

    #[test]
    fn rect_is_centred_and_fits_small_areas() {
        let r = picker_rect(Rect::new(0, 0, 100, 30), 5);
        assert!(r.x > 0 && r.right() < 100 && r.y > 0);
        let r = picker_rect(Rect::new(0, 0, 10, 4), 50);
        assert!(r.right() <= 10 && r.bottom() <= 4);
        assert_eq!(window_start(0, 5), 0);
        assert_eq!(window_start(9, 5), 5);
    }
}
