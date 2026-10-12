//! Mouse selection reaches the true end of the line and matches the highlight (task Bug 1).

mod common;
use common::ui::*;
use crossterm::event::{MouseButton, MouseEventKind};
use kuro::app::App;
use ratatui::buffer::Buffer as Cells;
use ratatui::style::Modifier;

fn cells_of(c: char, x: usize, tab: usize) -> usize {
    kuro::buffer::cell_width(c, x, tab)
}

/// Display width of `s` starting at column 0.
fn width(s: &str, tab: usize) -> usize {
    let mut x = 0;
    for c in s.chars() {
        x += cells_of(c, x, tab);
    }
    x
}

/// The cells of row `y` (text area only) that are drawn in reverse video.
fn reversed_cells(buf: &Cells, y: u16, from_x: u16) -> Vec<u16> {
    (from_x..buf.area.width).filter(|&x| buf[(x, y)].modifier.contains(Modifier::REVERSED)).collect()
}

fn drag(app: &mut App, from: (u16, u16), to: (u16, u16)) {
    mouse(app, MouseEventKind::Down(MouseButton::Left), from.0, from.1);
    mouse(app, MouseEventKind::Drag(MouseButton::Left), to.0, to.1);
    mouse(app, MouseEventKind::Up(MouseButton::Left), to.0, to.1);
}

/// Every highlighted cell on `row` belongs to the selection, and the selection's cells on that row are all highlighted.
fn assert_highlight_matches(app: &mut App, row: u16) {
    let gw = gutter_w(app);
    let buf = render(app);
    let hl = reversed_cells(&buf, row, gw);
    let line_idx = app.top_line + row as usize;
    let sel = app.selection_range().unwrap_or(0..0);
    let ls = app.buffer.line_start(line_idx);
    let text = app.buffer.line(line_idx);
    let le = ls + text.chars().count();
    let tab = app.cfg.editor.tab_width;
    // expected cells: for each selected char, its cells; plus one for a selected newline
    let mut want = vec![];
    let mut x = 0usize;
    for (i, c) in text.chars().enumerate() {
        let w = cells_of(c, x, tab);
        if sel.contains(&(ls + i)) {
            // a wide character is one terminal cell with the style (the terminal paints both columns with it)
            let shown = if c == '\t' { w } else { 1 };
            want.extend((x..x + shown).map(|v| gw + v as u16));
        }
        x += w;
    }
    if sel.contains(&le) && le < app.buffer.len_chars() {
        want.push(gw + x as u16);
    }
    assert_eq!(hl, want, "row {row}: highlighted cells vs selected cells (selection {sel:?})");
}

#[test]
fn dragging_from_the_start_to_past_the_end_selects_the_whole_line() {
    let (mut a, _) = app_with("hello world\nnext\n");
    let gw = gutter_w(&a);
    render(&mut a);
    drag(&mut a, (gw, 0), (gw + 40, 0));
    assert_eq!(a.buffer.selected_text().as_deref(), Some("hello world"));
    assert_highlight_matches(&mut a, 0);
}

#[test]
fn trailing_spaces_and_tabs_are_part_of_the_selection() {
    for line in ["text   ", "text\t", "text \t ", "text\t\t", "\t", "   "] {
        let (mut a, _) = app_with(&format!("{line}\nsecond\n"));
        let gw = gutter_w(&a);
        render(&mut a);
        drag(&mut a, (gw, 0), (gw + 60, 0));
        assert_eq!(a.buffer.selected_text().as_deref(), Some(line), "line {line:?}");
        assert_highlight_matches(&mut a, 0);
    }
}

#[test]
fn a_wide_character_at_the_end_is_selected_whole() {
    let (mut a, _) = app_with("ab日本\nxyz\n");
    let gw = gutter_w(&a);
    render(&mut a);
    drag(&mut a, (gw, 0), (gw + 30, 0));
    assert_eq!(a.buffer.selected_text().as_deref(), Some("ab日本"));
    assert_highlight_matches(&mut a, 0);
    assert_eq!(width("ab日本", 8), 6);
}

#[test]
fn dragging_across_lines_that_end_in_tabs_spaces_and_wide_characters() {
    let text = "one\t\ntwo  \n三四\nfour\n";
    let (mut a, _) = app_with(text);
    let gw = gutter_w(&a);
    render(&mut a);
    // from the start of line 1 to past the end of line 3
    drag(&mut a, (gw, 0), (gw + 50, 2));
    assert_eq!(a.buffer.selected_text().as_deref(), Some("one\t\ntwo  \n三四"));
    for row in 0..3 {
        assert_highlight_matches(&mut a, row);
    }
    // and bottom-up (a different start cell: not a double click)
    drag(&mut a, (gw + 50, 2), (gw, 0));
    assert_eq!(a.buffer.selected_text().as_deref(), Some("one\t\ntwo  \n三四"));
}

#[test]
fn the_last_character_is_included_when_the_mouse_is_over_its_right_half_or_beyond() {
    let (mut a, _) = app_with("abcdef\n");
    let gw = gutter_w(&a);
    render(&mut a);
    drag(&mut a, (gw, 0), (gw + 5, 0));
    assert_eq!(a.buffer.selected_text().as_deref(), Some("abcde"), "the mouse is on `f`: it is not included yet");
    drag(&mut a, (gw + 1, 0), (gw + 6, 0)); // another start cell: not a double click
    assert_eq!(a.buffer.selected_text().as_deref(), Some("bcdef"), "just past the end");
}

#[test]
fn it_is_the_same_in_insert_mode() {
    let (mut a, _) = modeless("hello\t \nnext\n");
    let gw = gutter_w(&a);
    render(&mut a);
    drag(&mut a, (gw, 0), (gw + 60, 0));
    assert_eq!(a.buffer.selected_text().as_deref(), Some("hello\t "));
    assert_highlight_matches(&mut a, 0);
}

#[test]
fn a_drag_that_leaves_the_text_area_keeps_extending_to_the_edge() {
    // the terminal reports the mouse at the very last column (the window edge)
    let (mut a, _) = app_with("abc def\nnext\n");
    let gw = gutter_w(&a);
    render(&mut a);
    mouse(&mut a, MouseEventKind::Down(MouseButton::Left), gw, 0);
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), 79, 0);
    assert_eq!(a.buffer.selected_text().as_deref(), Some("abc def"));
}

#[test]
fn a_drag_above_or_below_the_text_selects_to_the_first_or_last_line() {
    let (mut a, _) = app_with("one\ntwo\nthree\n");
    let gw = gutter_w(&a);
    render(&mut a);
    mouse(&mut a, MouseEventKind::Down(MouseButton::Left), gw + 1, 1);
    mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), gw + 60, 20); // far below the last line
    assert_eq!(
        a.buffer.selected_text().as_deref(),
        Some("wo\nthree\n"),
        "to the end of the last line (the empty line after the final newline)"
    );
}

#[test]
fn a_drag_ending_on_the_right_half_of_a_tab_or_wide_character_includes_it() {
    // a fresh editor per drag: two presses at one spot inside the double-click window would select a word
    let selected = |text: &str, to: u16| {
        let (mut a, _) = app_with(text);
        let gw = gutter_w(&a);
        render(&mut a);
        drag(&mut a, (gw, 0), (gw + to, 0));
        a.buffer.selected_text()
    };
    // the tab covers cells 2..8; cells 2-4 are its left half and 5-7 its right half
    assert_eq!(selected("ab\tcd\n", 4).as_deref(), Some("ab"), "left half: not yet");
    assert_eq!(selected("ab\tcd\n", 5).as_deref(), Some("ab\t"), "right half: the tab is selected");
    assert_eq!(selected("a日b\n", 1).as_deref(), Some("a"));
    assert_eq!(selected("a日b\n", 3).as_deref(), Some("a日"), "right cell of the wide character");
}
