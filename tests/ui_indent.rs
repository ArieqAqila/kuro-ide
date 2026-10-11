//! Indentation: Tab / Shift+Tab / Backspace / Delete with tabs, spaces, mixed whitespace and empty lines.

mod common;
use common::ui::*;
use kuro::app::App;
use kuro::config::Config;

fn with(style: &str, size: usize, tab_width: usize, text: &str) -> App {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    cfg.editor.indent_style = style.into();
    cfg.editor.indent_size = size;
    cfg.editor.tab_width = tab_width;
    app_cfg(text, cfg).0
}

fn tabs(text: &str) -> App {
    with("tabs", 8, 8, text)
}

fn select(app: &mut App, from: (usize, usize), to: (usize, usize)) {
    app.buffer.anchor = Some(app.buffer.pos_of(from.0, from.1));
    app.buffer.cursor = app.buffer.pos_of(to.0, to.1);
}

fn at(app: &mut App, line: usize, col: usize) {
    app.buffer.anchor = None;
    app.buffer.cursor = app.buffer.pos_of(line, col);
}

// ---------------------------------------------------------------- defaults and display

#[test]
fn defaults_are_hard_tabs_with_width_8() {
    let mut app = tabs("");
    press(&mut app, "tab");
    typed(&mut app, "x");
    assert_eq!(text_of(&app), "\tx");
    let gw = gutter_w(&app);
    let area = ratatui::layout::Rect::new(0, 0, W, H);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    let cur = app.render(&mut buf, area).unwrap();
    assert_eq!(cur.0, gw + 8 + 1, "a tab is displayed 8 cells wide");
}

#[test]
fn tab_width_changes_display_only() {
    let mut app = with("tabs", 8, 4, "\tx");
    let gw = gutter_w(&app);
    let s = screen(&mut app);
    assert_eq!(s[0].find('x').unwrap(), gw as usize + 4);
    assert_eq!(text_of(&app), "\tx");
}

// ---------------------------------------------------------------- Tab key

#[test]
fn tab_inserts_a_tab_or_spaces_to_the_next_stop() {
    let mut app = with("spaces", 4, 8, "ab");
    at(&mut app, 0, 2);
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "ab  ", "col 2 -> stop 4");
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "ab      ", "col 4 -> stop 8");
    let mut app = with("tabs", 4, 8, "ab");
    at(&mut app, 0, 1);
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "a\tb", "tabs style inserts a tab character regardless of indent_size");
}

#[test]
fn tab_with_multiline_selection_indents_every_nonempty_line() {
    let mut app = tabs("one\n\ntwo\nthree");
    select(&mut app, (0, 1), (2, 2));
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "\tone\n\n\ttwo\nthree", "empty line is left alone, line 4 untouched");
    assert_eq!(app.buffer.selected_text().as_deref(), Some("ne\n\n\ttw"), "selection still covers the same text");
}

#[test]
fn tab_with_spaces_style_indents_by_indent_size() {
    let mut app = with("spaces", 2, 8, "a\nb");
    select(&mut app, (0, 0), (1, 1));
    press(&mut app, "tab tab");
    assert_eq!(text_of(&app), "    a\n    b");
}

#[test]
fn selection_ending_at_column_zero_does_not_indent_that_last_line() {
    let mut app = tabs("a\nb\nc");
    select(&mut app, (0, 0), (2, 0));
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "\ta\n\tb\nc");
}

#[test]
fn single_line_partial_selection_is_replaced_by_a_tab_like_vscode() {
    let mut app = tabs("hello world");
    select(&mut app, (0, 0), (0, 5));
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "\t world");
}

#[test]
fn single_whole_line_selection_indents_the_line() {
    let mut app = tabs("hello\nworld");
    select(&mut app, (0, 0), (0, 5));
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "\thello\nworld");
}

#[test]
fn indenting_a_selection_is_one_undo_step_and_redoable() {
    let mut app = tabs("a\nb\nc");
    select(&mut app, (0, 0), (2, 1));
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "\ta\n\tb\n\tc");
    press(&mut app, "ctrl+z");
    assert_eq!(text_of(&app), "a\nb\nc", "one undo reverts all three lines");
    press(&mut app, "ctrl+y");
    assert_eq!(text_of(&app), "\ta\n\tb\n\tc");
    press(&mut app, "ctrl+z");
    typed(&mut app, "Z");
    press(&mut app, "ctrl+y");
    assert!(!text_of(&app).contains('\t'), "a new edit clears the redo history");
}

// ---------------------------------------------------------------- Shift+Tab

#[test]
fn shift_tab_without_selection_outdents_the_current_line_wherever_the_cursor_is() {
    let mut app = tabs("\t\tfoo bar");
    at(&mut app, 0, 7);
    press(&mut app, "shift+tab");
    assert_eq!(text_of(&app), "\tfoo bar");
    assert_eq!(app.buffer.cursor_line_col().1, 6, "cursor stays on the same character");
    press(&mut app, "shift+tab shift+tab");
    assert_eq!(text_of(&app), "foo bar", "nothing left to outdent: no-op");
}

#[test]
fn shift_tab_outdents_spaces_by_indent_size_and_handles_partial_indents() {
    let mut app = with("spaces", 4, 8, "      x\n  y\nz");
    select(&mut app, (0, 0), (2, 1));
    press(&mut app, "shift+tab");
    assert_eq!(text_of(&app), "  x\ny\nz");
}

#[test]
fn shift_tab_with_selection_outdents_all_selected_lines_only() {
    let mut app = tabs("\ta\n\tb\n\tc");
    select(&mut app, (0, 1), (1, 2));
    press(&mut app, "shift+tab");
    assert_eq!(text_of(&app), "a\nb\n\tc");
    press(&mut app, "ctrl+z");
    assert_eq!(text_of(&app), "\ta\n\tb\n\tc");
}

#[test]
fn shift_tab_on_empty_and_unindented_lines_is_a_no_op() {
    let mut app = tabs("\nplain");
    press(&mut app, "shift+tab");
    assert_eq!(text_of(&app), "\nplain");
    assert!(!app.buffer.is_modified(), "no edit was recorded");
}

// ---------------------------------------------------------------- Backspace

#[test]
fn backspace_in_leading_tab_indentation_removes_one_tab() {
    let mut app = tabs("\t\tfoo");
    at(&mut app, 0, 2);
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "\tfoo");
}

#[test]
fn backspace_in_leading_space_indentation_removes_one_indent_step() {
    let mut app = with("spaces", 4, 8, "        foo");
    at(&mut app, 0, 8);
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "    foo", "8 spaces, size 4: one keypress removes 4");
    let mut app = with("spaces", 4, 8, "      foo");
    at(&mut app, 0, 6);
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "    foo", "6 spaces: back to the previous stop (column 4)");
}

#[test]
fn backspace_with_mixed_indentation() {
    let mut app = with("spaces", 4, 8, "\t  foo");
    at(&mut app, 0, 3);
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "\tfoo", "the two spaces go together");
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "foo", "then the tab");
}

#[test]
fn backspace_removes_a_whole_whitespace_run_after_text() {
    let mut app = tabs("foo \t  bar");
    at(&mut app, 0, 7);
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "foobar");
    let mut app = tabs("a b");
    at(&mut app, 0, 2);
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "ab", "a single space is a run of one");
}

#[test]
fn backspace_at_line_start_joins_lines_and_works_on_empty_lines() {
    let mut app = tabs("a\n\nb");
    at(&mut app, 1, 0);
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "a\nb");
    press(&mut app, "home");
    at(&mut app, 0, 0);
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "a\nb", "start of file: nothing happens");
}

#[test]
fn backspace_deletes_the_selection_first() {
    let mut app = tabs("\t\tfoo");
    select(&mut app, (0, 0), (0, 3));
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "oo");
}

#[test]
fn smart_whitespace_delete_can_be_switched_off() {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    cfg.editor.smart_whitespace_delete = false;
    let (mut app, _) = app_cfg("foo   bar", cfg);
    at(&mut app, 0, 6);
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "foo  bar", "single char");
    press(&mut app, "delete");
    assert_eq!(text_of(&app), "foo  ar", "Delete removes one char too");
}

// ---------------------------------------------------------------- Delete

#[test]
fn delete_removes_the_whole_whitespace_run_in_one_keypress() {
    let mut app = tabs("foo\t\t  bar");
    at(&mut app, 0, 3);
    press(&mut app, "delete");
    assert_eq!(text_of(&app), "foobar");
}

#[test]
fn delete_in_leading_indentation_removes_all_of_it() {
    let mut app = tabs("\t \t  foo");
    at(&mut app, 0, 0);
    press(&mut app, "delete");
    assert_eq!(text_of(&app), "foo");
}

#[test]
fn delete_on_text_removes_one_char_and_at_line_end_joins_lines() {
    let mut app = tabs("abc\ndef");
    at(&mut app, 0, 1);
    press(&mut app, "delete");
    assert_eq!(text_of(&app), "ac\ndef");
    at(&mut app, 0, 2);
    press(&mut app, "delete");
    assert_eq!(text_of(&app), "acdef", "end of line joins the next line");
    at(&mut app, 0, 5);
    press(&mut app, "delete");
    assert_eq!(text_of(&app), "acdef", "end of file: no-op");
}

#[test]
fn delete_on_whitespace_only_and_empty_lines() {
    let mut app = tabs("   \n\nx");
    at(&mut app, 0, 0);
    press(&mut app, "delete");
    assert_eq!(text_of(&app), "\n\nx");
    at(&mut app, 1, 0);
    press(&mut app, "delete");
    assert_eq!(text_of(&app), "\nx", "empty line: joins");
}

#[test]
fn wide_characters_are_deleted_one_at_a_time() {
    let mut app = tabs("日本語");
    at(&mut app, 0, 2);
    press(&mut app, "backspace");
    assert_eq!(text_of(&app), "日語");
    press(&mut app, "delete");
    assert_eq!(text_of(&app), "日");
}

// ---------------------------------------------------------------- modal mode

#[test]
fn tab_in_insert_mode_of_modal_editing_works_and_normal_mode_ignores_it() {
    let (mut app, _) = app_with("x");
    press(&mut app, "tab");
    assert_eq!(text_of(&app), "x", "Normal mode: Tab does not edit");
    press(&mut app, "i tab");
    assert_eq!(text_of(&app), "\tx");
}

#[test]
fn enter_keeps_the_indentation_style_of_the_current_line() {
    let mut app = tabs("\t\tfoo");
    at(&mut app, 0, 5);
    press(&mut app, "enter");
    assert_eq!(text_of(&app), "\t\tfoo\n\t\t");
}
