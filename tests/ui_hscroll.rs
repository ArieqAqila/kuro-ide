//! Horizontal scrolling with the mouse (task Bug 3): the terminal's horizontal wheel (buttons 6 and 7) and
//! Shift + vertical wheel as the fallback, clamped between the start and the end of the longest line.

mod common;
use common::ui::*;
use crossterm::event::{KeyModifiers, MouseEventKind};
use kuro::app::App;

fn long_lines() -> String {
    // the longest line is 200 cells
    format!("short\n{}\nmiddle line\n", "x".repeat(200))
}

fn app(text: &str) -> (App, u16, u16) {
    let (mut a, _) = app_with(text);
    render(&mut a);
    let gw = gutter_w(&a);
    (a, gw, 5)
}

fn right(a: &mut App, gw: u16) {
    mouse(a, MouseEventKind::ScrollRight, gw + 2, 1);
}
fn left(a: &mut App, gw: u16) {
    mouse(a, MouseEventKind::ScrollLeft, gw + 2, 1);
}

#[test]
fn horizontal_wheel_events_scroll_right_and_left() {
    let (mut a, gw, _) = app(&long_lines());
    assert_eq!(a.left_col, 0);
    right(&mut a, gw);
    let step = a.left_col;
    assert!(step > 0, "scrolled right");
    right(&mut a, gw);
    assert_eq!(a.left_col, step * 2);
    left(&mut a, gw);
    assert_eq!(a.left_col, step);
    // the screen follows: the first column shown is now `step` cells in
    let row = screen(&mut a);
    assert!(row[1].contains('x'), "{:?}", row[1]);
    assert!(!row[0].contains("short"), "the short line has scrolled out of view: {:?}", row[0]);
}

#[test]
fn shift_plus_the_vertical_wheel_is_the_fallback() {
    let (mut a, gw, _) = app(&long_lines());
    mouse_mod(&mut a, MouseEventKind::ScrollDown, gw + 2, 1, KeyModifiers::SHIFT);
    let step = a.left_col;
    assert!(step > 0, "Shift+wheel down scrolls right");
    mouse_mod(&mut a, MouseEventKind::ScrollDown, gw + 2, 1, KeyModifiers::SHIFT);
    assert_eq!(a.left_col, step * 2);
    mouse_mod(&mut a, MouseEventKind::ScrollUp, gw + 2, 1, KeyModifiers::SHIFT);
    assert_eq!(a.left_col, step, "Shift+wheel up scrolls left");
    assert_eq!(a.top_line, 0, "and the view did not move vertically");
}

#[test]
fn it_is_clamped_at_the_start() {
    let (mut a, gw, _) = app(&long_lines());
    left(&mut a, gw);
    assert_eq!(a.left_col, 0);
    right(&mut a, gw);
    for _ in 0..5 {
        left(&mut a, gw);
    }
    assert_eq!(a.left_col, 0);
}

#[test]
fn it_is_clamped_at_the_end_of_the_longest_line() {
    let (mut a, gw, _) = app(&long_lines());
    for _ in 0..200 {
        right(&mut a, gw);
    }
    let avail = (W - gw) as usize;
    assert_eq!(a.left_col, 200 - avail, "the end of the longest line is at the right edge");
    // and nothing more happens when scrolling on
    right(&mut a, gw);
    assert_eq!(a.left_col, 200 - avail);
    // the same for Shift + wheel
    for _ in 0..200 {
        mouse_mod(&mut a, MouseEventKind::ScrollDown, gw + 2, 1, KeyModifiers::SHIFT);
    }
    assert_eq!(a.left_col, 200 - avail);
    let row = screen(&mut a);
    assert!(row[1].trim_end().ends_with('x'), "the end of the long line is visible");
}

#[test]
fn nothing_scrolls_when_everything_fits() {
    let (mut a, gw, _) = app("short\nlines only\n");
    right(&mut a, gw);
    mouse_mod(&mut a, MouseEventKind::ScrollDown, gw + 2, 0, KeyModifiers::SHIFT);
    assert_eq!(a.left_col, 0);
}

#[test]
fn tabs_count_by_their_display_width() {
    // 20 tabs = 160 cells
    let (mut a, gw, _) = app(&format!("{}\n", "\t".repeat(20)));
    for _ in 0..100 {
        right(&mut a, gw);
    }
    assert_eq!(a.left_col, 160 - (W - gw) as usize);
}

#[test]
fn the_longest_line_is_found_again_after_an_edit() {
    let (mut a, gw, _) = app("ab\ncd\n");
    right(&mut a, gw);
    assert_eq!(a.left_col, 0);
    a.buffer.insert(0, &"y".repeat(150));
    render(&mut a);
    for _ in 0..100 {
        right(&mut a, gw);
    }
    assert_eq!(a.left_col, 152 - (W - gw) as usize);
}

#[test]
fn the_view_shows_a_scrolled_selection_and_clicks_map_to_the_scrolled_text() {
    let (mut a, gw, _) = app(&long_lines());
    for _ in 0..3 {
        right(&mut a, gw);
    }
    let col = a.left_col;
    click(&mut a, gw + 4, 1);
    assert_eq!(a.buffer.cursor_line_col(), (1, col + 4));
}
