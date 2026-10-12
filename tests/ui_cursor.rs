//! Cursor movement: the column kept for vertical moves is a *display* column (tabs expanded, wide characters
//! counted by width), not a character index. Regression tests for task Bug 5.

mod common;
use common::ui::*;
use kuro::config::Config;

fn with_tab(text: &str, tab: usize) -> kuro::app::App {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    cfg.editor.tab_width = tab;
    app_cfg(text, cfg).0
}

fn at(app: &mut kuro::app::App, line: usize, col: usize) {
    app.buffer.cursor = app.buffer.pos_of(line, col);
    app.buffer.set_goal();
}

fn lc(app: &kuro::app::App) -> (usize, usize) {
    app.buffer.cursor_line_col()
}

#[test]
fn regression_down_after_a_tab_separated_field_keeps_the_display_column() {
    // the report: the separator is a tab; the cursor just after `DO` must land just after `DO` below
    let mut a = with_tab("ldsbjml5\tDO_CALC\nldsbkode14\tDO_CALC\n", 8);
    at(&mut a, 0, 11); // after "ldsbjml5<TAB>DO"
    assert_eq!(a.buffer.slice(a.buffer.cursor - 2..a.buffer.cursor), "DO");
    press(&mut a, "down");
    assert_eq!(lc(&a), (1, 13), "after `ldsbkode14<TAB>DO`");
    assert_eq!(a.buffer.slice(a.buffer.cursor - 2..a.buffer.cursor), "DO");
    press(&mut a, "up");
    assert_eq!(lc(&a), (0, 11), "and back again");
}

#[test]
fn it_works_for_other_tab_widths() {
    for tab in [2, 3, 4, 8] {
        let mut a = with_tab("ab\tX\nabcdef\tX\n", tab);
        at(&mut a, 0, 3); // just after the tab
        let want = a.buffer.display_col(a.buffer.cursor);
        press(&mut a, "down");
        let got = a.buffer.display_col(a.buffer.cursor);
        // the second line may have no character boundary exactly there: it is the nearest one at or around it
        assert!(got.abs_diff(want) <= tab.div_ceil(2), "tab {tab}: wanted column {want}, got {got}");
    }
    // tab 4: "ab\t" ends at cell 4; line 2 "abcd" has a boundary there
    let mut a = with_tab("ab\tX\nabcdefX\n", 4);
    at(&mut a, 0, 3);
    press(&mut a, "down");
    assert_eq!(lc(&a), (1, 4));
}

#[test]
fn a_goal_column_inside_a_tab_goes_to_the_nearer_edge() {
    let mut a = with_tab("\t\nabcdefghijkl\n", 8);
    at(&mut a, 1, 3);
    press(&mut a, "up");
    assert_eq!(lc(&a), (0, 0), "cell 3 of an 8-cell tab is nearer its left edge");
    let mut a = with_tab("\t\nabcdefghijkl\n", 8);
    at(&mut a, 1, 6);
    press(&mut a, "up");
    assert_eq!(lc(&a), (0, 1), "cell 6 is nearer the right edge");
}

#[test]
fn longer_and_shorter_lines_clamp_and_the_column_comes_back() {
    let mut a = with_tab("0123456789\nab\n0123456789\n", 8);
    at(&mut a, 0, 7);
    press(&mut a, "down");
    assert_eq!(lc(&a), (1, 2), "short line: clamped to its end");
    press(&mut a, "down");
    assert_eq!(lc(&a), (2, 7), "the desired column is kept across consecutive moves");
    press(&mut a, "up up");
    assert_eq!(lc(&a), (0, 7));
}

#[test]
fn wide_characters_count_as_two_cells() {
    // "日本語" is 6 cells; after two of them the cursor is at cell 4
    let mut a = with_tab("日本語xyz\nabcdefgh\n", 8);
    at(&mut a, 0, 2);
    assert_eq!(a.buffer.display_col(a.buffer.cursor), 4);
    press(&mut a, "down");
    assert_eq!(lc(&a), (1, 4));
    // and the other way: from ASCII to wide text, cell 3 is inside 本 (cells 2-3): nearest edge is cell 2 or 4
    let mut a = with_tab("abcdefgh\n日本語xyz\n", 8);
    at(&mut a, 0, 3);
    press(&mut a, "down");
    assert_eq!(a.buffer.display_col(a.buffer.cursor) % 2, 0, "never lands in the middle of a wide character");
    assert_eq!(lc(&a), (1, 1), "tie goes to the left edge");
}

#[test]
fn empty_lines_are_passed_through_without_losing_the_column() {
    let mut a = with_tab("abcdef\n\nabcdef\n", 8);
    at(&mut a, 0, 4);
    press(&mut a, "down");
    assert_eq!(lc(&a), (1, 0));
    press(&mut a, "down");
    assert_eq!(lc(&a), (2, 4), "the column survives an empty line");
}

#[test]
fn the_goal_resets_on_horizontal_movement_and_on_edits() {
    let mut a = with_tab("0123456789\nab\n0123456789\n", 8);
    at(&mut a, 0, 8);
    press(&mut a, "down");
    assert_eq!(lc(&a), (1, 2));
    press(&mut a, "left");
    press(&mut a, "down");
    assert_eq!(lc(&a), (2, 1), "after a horizontal move the goal is where the cursor is");
    at(&mut a, 0, 8);
    press(&mut a, "down");
    typed(&mut a, "Z");
    press(&mut a, "down");
    assert_eq!(lc(&a), (2, 3), "typing resets the goal to the cursor");
}

#[test]
fn page_up_and_page_down_use_the_display_column_too() {
    let text: String = (0..60).map(|i| if i % 2 == 0 { "ab\tcd\n".to_string() } else { "abcdefghcd\n".to_string() }).collect();
    let mut a = with_tab(&text, 8);
    at(&mut a, 0, 4); // after "ab<TAB>c": cell 9
    press(&mut a, "pagedown");
    let (l, _) = lc(&a);
    assert!(l > 1);
    assert_eq!(a.buffer.display_col(a.buffer.cursor), 9.min(a.buffer.display_col(a.buffer.line_end(l))));
}

#[test]
fn normal_mode_j_and_k_keep_the_display_column_too() {
    let (mut a, _) = app_with("ldsbjml5\tDO_CALC\nldsbkode14\tDO_CALC\n");
    a.buffer.cursor = a.buffer.pos_of(0, 10); // on the `O` of DO (normal mode keeps the cursor on a character)
    a.buffer.set_goal();
    typed(&mut a, "j");
    assert_eq!(a.buffer.cursor_line_col(), (1, 12), "on the `O` of the second line's DO");
    typed(&mut a, "k");
    assert_eq!(a.buffer.cursor_line_col(), (0, 10));
}
