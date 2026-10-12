//! Ctrl + right-click on whitespace moves the cursor to the end of the whitespace run (task Bug 4).
//!
//! Rule: whitespace is a space or a tab; a run is the maximal stretch of those on one line, so a mixed
//! "  \t " is a single run. Clicking anywhere on the run (first cell, middle, last cell, any cell of a tab)
//! puts the cursor just after it, i.e. on the next non-whitespace character, or at the end of the line when the
//! run is trailing. Clicking anything else just places the cursor under the mouse.

mod common;
use common::ui::*;
use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
use kuro::app::App;

fn ctrl_right_click(a: &mut App, x: u16, y: u16) {
    mouse_mod(a, MouseEventKind::Down(MouseButton::Right), x, y, KeyModifiers::CONTROL);
    mouse_mod(a, MouseEventKind::Up(MouseButton::Right), x, y, KeyModifiers::CONTROL);
}

fn setup(text: &str, modal: bool) -> (App, u16) {
    let mut a = if modal { app_with(text).0 } else { modeless(text).0 };
    let gw = gutter_w(&a);
    render(&mut a);
    (a, gw)
}

#[test]
fn a_run_of_spaces_from_start_middle_and_end() {
    // "ab" then five spaces (cells 2..7) then "cd"
    for cell in [2u16, 4, 6] {
        let (mut a, gw) = setup("ab     cd\n", true);
        ctrl_right_click(&mut a, gw + cell, 0);
        assert_eq!(a.buffer.cursor_line_col(), (0, 7), "clicked cell {cell}: just before `cd`");
    }
}

#[test]
fn a_run_of_tabs() {
    // two tabs: cells 2..16; any cell of either tab is part of the run
    for cell in [2u16, 7, 8, 15] {
        let (mut a, gw) = setup("ab\t\tcd\n", true);
        ctrl_right_click(&mut a, gw + cell, 0);
        assert_eq!(a.buffer.cursor_line_col(), (0, 4), "clicked cell {cell}");
    }
}

#[test]
fn a_mixed_run_of_spaces_and_tabs_is_one_run() {
    // "ab", space, tab, space, space, "cd": 6 whitespace chars at indexes 2..7
    for cell in [2u16, 3, 8, 9, 10] {
        let (mut a, gw) = setup("ab \t  cd\n", true);
        ctrl_right_click(&mut a, gw + cell, 0);
        assert_eq!(a.buffer.cursor_line_col(), (0, 6), "clicked cell {cell}");
    }
}

#[test]
fn clicking_the_last_whitespace_of_a_run_and_a_click_right_after_it() {
    let (mut a, gw) = setup("a   b\n", true);
    ctrl_right_click(&mut a, gw + 3, 0); // the last space
    assert_eq!(a.buffer.cursor_line_col(), (0, 4));
    let (mut a, gw) = setup("a   b\n", true);
    ctrl_right_click(&mut a, gw + 4, 0); // on `b`: not whitespace, cursor goes there
    assert_eq!(a.buffer.cursor_line_col(), (0, 4));
}

#[test]
fn trailing_whitespace_goes_to_the_end_of_the_line() {
    let (mut a, gw) = setup("abc \t \nnext\n", false);
    ctrl_right_click(&mut a, gw + 3, 0);
    assert_eq!(a.buffer.cursor_line_col(), (0, 6), "insert mode: past the last whitespace character");
    let (mut a, gw) = setup("abc \t \nnext\n", true);
    ctrl_right_click(&mut a, gw + 3, 0);
    assert_eq!(a.buffer.cursor_line_col(), (0, 5), "normal mode keeps the cursor on the last character");
}

#[test]
fn leading_indentation_is_a_run_too() {
    let (mut a, gw) = setup("\t  x = 1\n", true);
    ctrl_right_click(&mut a, gw + 1, 0);
    assert_eq!(a.buffer.cursor_line_col(), (0, 3), "on `x`");
}

#[test]
fn a_run_never_crosses_a_line_break() {
    let (mut a, gw) = setup("ab  \n  cd\n", true);
    ctrl_right_click(&mut a, gw + 2, 0);
    assert_eq!(a.buffer.cursor_line_col().0, 0);
}

#[test]
fn clicking_a_non_whitespace_character_does_not_regress() {
    let (mut a, gw) = setup("hello world\n", true);
    ctrl_right_click(&mut a, gw + 7, 0);
    assert_eq!(a.buffer.cursor_line_col(), (0, 7), "the cursor goes under the mouse");
    assert_eq!(a.buffer.selection(), None);
}

#[test]
fn it_clears_a_selection_and_a_plain_right_click_changes_nothing() {
    let (mut a, gw) = setup("ab   cd\n", true);
    press(&mut a, "shift+right shift+right");
    assert!(a.buffer.selection().is_some());
    mouse(&mut a, MouseEventKind::Down(MouseButton::Right), gw + 3, 0);
    assert!(a.buffer.selection().is_some(), "a plain right-click is not bound to anything");
    ctrl_right_click(&mut a, gw + 3, 0);
    assert_eq!((a.buffer.selection(), a.buffer.cursor_line_col()), (None, (0, 5)));
}

#[test]
fn the_view_scrolled_sideways_still_maps_the_click_correctly() {
    let (mut a, gw) = setup(&format!("{}   end\n", "x".repeat(100)), true);
    a.buffer.cursor = 104; // the view follows the cursor, so keep it inside the scrolled window
    a.left_col = 90;
    render(&mut a);
    assert!(a.left_col > 0, "the view is scrolled");
    let shift = a.left_col as u16;
    ctrl_right_click(&mut a, gw + (101 - shift), 0); // cell 101: a space
    assert_eq!(a.buffer.cursor_line_col(), (0, 103));
}
