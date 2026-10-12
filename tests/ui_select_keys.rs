//! Shift + movement keys start and extend a selection in normal mode, as in insert mode and VS Code (task Bug 2).

mod common;
use common::ui::*;
use kuro::app::App;

const TEXT: &str = "alpha beta gamma\nsecond line here\nthird\n";

fn normal() -> App {
    let (mut a, _) = app_with(TEXT);
    assert_eq!(format!("{:?}", a.mode), "Normal");
    a.buffer.cursor = a.buffer.pos_of(1, 7);
    a.buffer.set_goal();
    a
}

fn sel(a: &App) -> Option<(usize, usize)> {
    a.buffer.selection().map(|r| (r.start, r.end))
}

fn at(a: &App, line: usize, col: usize) -> usize {
    a.buffer.pos_of(line, col)
}

#[test]
fn shift_right_and_left_start_a_selection() {
    let mut a = normal();
    press(&mut a, "shift+right");
    assert_eq!(sel(&a), Some((at(&a, 1, 7), at(&a, 1, 8))));
    let mut a = normal();
    press(&mut a, "shift+left");
    assert_eq!(sel(&a), Some((at(&a, 1, 6), at(&a, 1, 7))));
}

#[test]
fn shift_up_and_down_select_across_lines() {
    let mut a = normal();
    press(&mut a, "shift+down");
    assert_eq!(sel(&a), Some((at(&a, 1, 7), at(&a, 2, 5))), "clamped to the short last line");
    let mut a = normal();
    press(&mut a, "shift+up");
    assert_eq!(sel(&a), Some((at(&a, 0, 7), at(&a, 1, 7))));
}

#[test]
fn shift_home_and_end_select_to_the_line_edges() {
    let mut a = normal();
    press(&mut a, "shift+end");
    assert_eq!(sel(&a), Some((at(&a, 1, 7), at(&a, 1, 16))), "End goes to the true end of the line");
    let mut a = normal();
    press(&mut a, "shift+home");
    assert_eq!(sel(&a), Some((at(&a, 1, 0), at(&a, 1, 7))));
}

#[test]
fn shift_ctrl_arrows_select_by_word() {
    let mut a = normal();
    press(&mut a, "shift+ctrl+right");
    assert_eq!(sel(&a), Some((at(&a, 1, 7), at(&a, 1, 12))), "to the start of the next word");
    let mut a = normal();
    press(&mut a, "shift+ctrl+left");
    assert_eq!(sel(&a), Some((at(&a, 1, 0), at(&a, 1, 7))), "to the start of the word before");
}

#[test]
fn shift_page_up_and_down_select_a_page() {
    let text: String = (0..100).map(|i| format!("line {i}\n")).collect();
    let (mut a, _) = app_with(&text);
    press(&mut a, "shift+pagedown");
    let (s, e) = sel(&a).expect("a selection");
    assert_eq!(s, 0);
    assert!(a.buffer.line_of(e) >= 10, "moved down by about a page");
    press(&mut a, "shift+pageup");
    assert_eq!(sel(&a), None, "back at the anchor: nothing selected");
}

#[test]
fn an_existing_selection_extends_in_both_directions_and_shrinks_back() {
    let mut a = normal();
    press(&mut a, "shift+right shift+right shift+right");
    assert_eq!(sel(&a), Some((at(&a, 1, 7), at(&a, 1, 10))));
    press(&mut a, "shift+left");
    assert_eq!(sel(&a), Some((at(&a, 1, 7), at(&a, 1, 9))), "shrinks");
    press(&mut a, "shift+left shift+left shift+left");
    assert_eq!(sel(&a), Some((at(&a, 1, 6), at(&a, 1, 7))), "crosses the anchor and extends the other way");
}

#[test]
fn a_plain_arrow_clears_the_selection_and_esc_too() {
    let mut a = normal();
    press(&mut a, "shift+right shift+right");
    press(&mut a, "right");
    assert_eq!(sel(&a), None);
    press(&mut a, "shift+right");
    assert!(sel(&a).is_some());
    press(&mut a, "esc");
    assert_eq!(sel(&a), None);
}

#[test]
fn the_selection_is_what_the_next_command_acts_on() {
    let mut a = normal();
    press(&mut a, "shift+right shift+right shift+right");
    assert_eq!(a.buffer.selected_text().as_deref(), Some("lin"));
    typed(&mut a, "y");
    assert_eq!(sel(&a), None, "yank finished");
}
