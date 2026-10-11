//! UI tests on the cell buffer (no pty): what is painted, and how input changes it.
//! See README "Testing" for the gap between this layer and a real terminal.

mod common;
use common::ui::*;
use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
use kuro::config::Config;
use ratatui::style::{Color, Modifier};

const SAMPLE: &str = "alpha beta\nfoo bar foo\nSELECT 1;\nFoo and foo\nlast line here";

fn all_cells(buf: &ratatui::buffer::Buffer) -> impl Iterator<Item = &ratatui::buffer::Cell> {
    buf.content.iter()
}

// ---------------------------------------------------------------- background

#[test]
fn background_is_never_painted_anywhere() {
    let (mut app, _) = app_with(SAMPLE);
    // exercise every kind of UI element at once: selection, find bar with matches, replace row
    press(&mut app, "ctrl+a");
    press(&mut app, "ctrl+h");
    typed(&mut app, "foo");
    let buf = render(&mut app);
    assert!(all_cells(&buf).any(|c| c.modifier.contains(Modifier::REVERSED)), "test must actually exercise reverse video");
    for (i, c) in all_cells(&buf).enumerate() {
        assert_eq!(c.bg, Color::Reset, "cell {i} ({:?}) forces a background", c.symbol());
    }
}

#[test]
fn only_ansi_palette_colors_are_used() {
    let (mut app, _) = app_with("");
    app.open_file(std::path::Path::new("x.json"));
    press(&mut app, "i");
    typed(&mut app, "{\"k\": [1, true, null, \"s\"]} ");
    let mut cfg_sql = app_with("SELECT 'a' -- c\n/* x */ FROM t WHERE n = 12");
    cfg_sql.0.lang = &kuro::syntax::SQL;
    let buf = render(&mut cfg_sql.0);
    let mut seen = std::collections::BTreeSet::new();
    for c in buf.content.iter().chain(render(&mut app).content.iter()) {
        seen.insert(format!("{:?}", c.fg));
        assert!(!matches!(c.fg, Color::Rgb(..) | Color::Indexed(_)), "hardcoded color {:?}", c.fg);
        assert_eq!(c.bg, Color::Reset);
    }
    assert!(seen.len() > 2, "syntax highlighting should use several ANSI colors: {seen:?}");
}

#[test]
fn background_untouched_in_sql_pane_and_error_pane_too() {
    let (mut app, _) = app_with("select 1");
    common::sqlview::show_rows(&mut app, 30, 6);
    press(&mut app, "f4");
    for cell in all_cells(&render(&mut app)) {
        assert_eq!(cell.bg, Color::Reset);
    }
    common::sqlview::show_error(&mut app, "syntax", "syntax error at or near \"SELEC\"", Some(1), Some(1));
    for cell in all_cells(&render(&mut app)) {
        assert_eq!(cell.bg, Color::Reset);
    }
}

// ---------------------------------------------------------------- gutter & status

#[test]
fn gutter_shows_absolute_line_numbers_by_default() {
    let (mut app, _) = app_with(SAMPLE);
    press(&mut app, "j j"); // cursor on line 3
    let s = screen(&mut app);
    for (i, row) in s.iter().take(5).enumerate() {
        assert!(row.starts_with(&format!(" {} ", i + 1)), "row {i}: {row:?}");
    }
    assert!(s[0].starts_with(" 1 alpha"), "{:?}", s[0]);
}

#[test]
fn relative_numbers_are_opt_in_and_hybrid() {
    let mut cfg = Config::default();
    cfg.editor.relative_numbers = true;
    let (mut app, _) = app_cfg(SAMPLE, cfg);
    press(&mut app, "j j");
    let s = screen(&mut app);
    assert!(s[0].starts_with(" 2 alpha"), "{:?}", s[0]);
    assert!(s[1].starts_with(" 1 foo"), "{:?}", s[1]);
    assert!(s[2].starts_with(" 3 SELECT"), "current line shows its absolute number: {:?}", s[2]);
    assert!(s[3].starts_with(" 1 Foo"), "{:?}", s[3]);
    assert!(s[4].starts_with(" 2 last"), "{:?}", s[4]);
}

#[test]
fn current_line_number_is_highlighted() {
    let (mut app, _) = app_with(SAMPLE);
    press(&mut app, "j");
    let buf = render(&mut app);
    let cur = &buf[(1, 1)];
    assert_eq!(cur.symbol(), "2");
    assert!(cur.modifier.contains(Modifier::BOLD));
    assert!(!buf[(1, 0)].modifier.contains(Modifier::BOLD));
    assert!(buf[(1, 0)].modifier.contains(Modifier::DIM));
}

#[test]
fn gutter_width_grows_at_10_100_and_1000_lines_and_is_stable_while_scrolling() {
    let width = |n: usize| {
        let text = vec!["x"; n].join("\n");
        let (app, _) = app_with(&text);
        gutter_w(&app)
    };
    assert_eq!(width(1), 3);
    assert_eq!(width(9), 3);
    assert_eq!(width(10), 4, "10 lines needs two digits");
    assert_eq!(width(99), 4);
    assert_eq!(width(100), 5);
    assert_eq!(width(999), 5);
    assert_eq!(width(1000), 6);
    let text = vec!["x"; 1000].join("\n");
    let (mut app, _) = app_with(&text);
    let before = gutter_w(&app);
    let top = screen(&mut app)[0].clone();
    press(&mut app, "G");
    let bottom = screen(&mut app);
    assert_eq!(gutter_w(&app), before);
    assert_eq!(top.find('x'), bottom[0].find('x'), "text column does not shift");
    assert!(bottom.iter().any(|r| r.trim_start().starts_with("1000 x")));
}

#[test]
fn empty_file_shows_one_line() {
    let (mut app, _) = app_with("");
    let s = screen(&mut app);
    assert_eq!(s[0].trim_end(), " 1");
    assert_eq!(s[1].trim_end(), "", "no phantom second line");
    assert_eq!(app.buffer.line_count(), 1);
}

#[test]
fn line_numbers_can_be_turned_off() {
    let mut cfg = Config::default();
    cfg.editor.line_numbers = false;
    let (mut app, _) = app_cfg(SAMPLE, cfg);
    assert!(screen(&mut app)[0].starts_with("alpha beta"));
}

#[test]
fn status_bar_shows_file_modified_position_and_type() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("q.sql");
    std::fs::write(&p, "select 1;\nselect 2;\n").unwrap();
    let (mut app, _) = app_with("");
    app.open_file(&p);
    let s = screen(&mut app);
    let status = &s[22];
    assert!(status.contains("NORMAL"), "{status:?}");
    assert!(status.contains("q.sql"), "{status:?}");
    assert!(!status.contains("[+]"));
    assert!(status.contains("Ln 1, Col 1"), "{status:?}");
    assert!(status.contains("SQL"), "{status:?}");
    press(&mut app, "A");
    typed(&mut app, "x");
    let status = screen(&mut app)[22].clone();
    assert!(status.contains("INSERT") && status.contains("[+]"), "{status:?}");
    assert!(status.contains("Ln 1, Col 11"), "{status:?}");
}

// ---------------------------------------------------------------- find bar

#[test]
fn ctrl_f_opens_find_bar_in_the_top_right() {
    let (mut app, _) = app_with(SAMPLE);
    press(&mut app, "ctrl+f");
    let buf = render(&mut app);
    let (x, y) = find_on(&buf, "Find").expect("find bar placeholder");
    assert!(y <= 1, "bar is at the top, got row {y}");
    assert!(x > W / 3, "bar is on the right side, got column {x}");
    let (cx, _) = find_on(&buf, "✕").unwrap();
    assert!(cx >= W - 8, "close button hugs the right edge: {cx}");
}

#[test]
fn typing_shows_match_count_and_selects_first_match() {
    let (mut app, _) = app_with(SAMPLE);
    press(&mut app, "ctrl+f");
    typed(&mut app, "foo");
    let s = screen(&mut app).join("\n");
    assert!(s.contains("1 of 4"), "{s}");
    assert_eq!(app.buffer.selected_text().as_deref(), Some("foo"));
}

#[test]
fn enter_and_shift_enter_cycle_matches_with_wraparound() {
    let (mut app, _) = app_with(SAMPLE);
    press(&mut app, "ctrl+f");
    typed(&mut app, "foo");
    let label = |app: &mut kuro::app::App| {
        let s = screen(app).join("\n");
        ["1 of 4", "2 of 4", "3 of 4", "4 of 4"].into_iter().find(|l| s.contains(l)).unwrap_or("?").to_string()
    };
    assert_eq!(label(&mut app), "1 of 4");
    press(&mut app, "enter");
    assert_eq!(label(&mut app), "2 of 4");
    press(&mut app, "enter enter");
    assert_eq!(label(&mut app), "4 of 4");
    press(&mut app, "enter");
    assert_eq!(label(&mut app), "1 of 4", "wraps forward");
    press(&mut app, "shift+enter");
    assert_eq!(label(&mut app), "4 of 4", "wraps backward");
    press(&mut app, "f3");
    assert_eq!(label(&mut app), "1 of 4", "F3 = next");
    press(&mut app, "shift+f3");
    assert_eq!(label(&mut app), "4 of 4", "Shift+F3 = previous");
}

#[test]
fn toggles_change_the_matches_and_show_as_active() {
    let (mut app, _) = app_with("Foo foo FOO foobar");
    press(&mut app, "ctrl+f");
    typed(&mut app, "foo");
    assert!(screen(&mut app).join("\n").contains("1 of 4"));

    press(&mut app, "alt+c"); // match case: "foo" and the one inside "foobar"
    assert!(screen(&mut app).join("\n").contains("1 of 2"));
    let buf = render(&mut app);
    let (x, y) = find_on(&buf, "Aa").unwrap();
    assert!(buf[(x, y)].modifier.contains(Modifier::REVERSED), "active toggle is reverse video");
    press(&mut app, "alt+c");
    assert!(screen(&mut app).join("\n").contains("of 4"));

    press(&mut app, "alt+w"); // whole word
    assert!(screen(&mut app).join("\n").contains("of 3"));
    let buf = render(&mut app);
    let (x, y) = find_on(&buf, "ab").unwrap();
    assert!(buf[(x, y)].modifier.contains(Modifier::REVERSED));
    press(&mut app, "alt+w");

    press(&mut app, "alt+r"); // regex
    for _ in 0..3 {
        press(&mut app, "backspace");
    }
    typed(&mut app, "fo+");
    assert!(screen(&mut app).join("\n").contains("of 4"));
    typed(&mut app, "(");
    assert!(screen(&mut app).join("\n").contains("Bad regex"));
}

#[test]
fn no_results_is_reported() {
    let (mut app, _) = app_with(SAMPLE);
    press(&mut app, "ctrl+f");
    typed(&mut app, "zzz");
    assert!(screen(&mut app).join("\n").contains("No results"));
}

#[test]
fn all_matches_are_highlighted() {
    let (mut app, _) = app_with("\n\n\n\nfoo x foo y foo");
    press(&mut app, "ctrl+f");
    typed(&mut app, "foo");
    press(&mut app, "enter"); // current = 2nd
    let buf = render(&mut app);
    let marked: Vec<u16> = (5..20)
        .filter(|x| buf[(*x, 4)].modifier.contains(Modifier::UNDERLINED) || buf[(*x, 4)].modifier.contains(Modifier::REVERSED))
        .collect();
    assert!(marked.contains(&5) && marked.contains(&11) && marked.contains(&17), "{marked:?}");
}

#[test]
fn esc_closes_the_bar_and_keeps_focus_in_the_editor() {
    let (mut app, _) = app_with(SAMPLE);
    press(&mut app, "ctrl+f");
    assert!(find_on(&render(&mut app), "Find").is_some());
    press(&mut app, "esc");
    assert!(find_on(&render(&mut app), "Find").is_none());
    press(&mut app, "i");
    typed(&mut app, "Z");
    assert!(text_of(&app).starts_with('Z') || text_of(&app).contains('Z'));
}

#[test]
fn ctrl_h_opens_replace_and_replaces_one_or_all() {
    let (mut app, _) = app_with("cat dog cat dog cat");
    press(&mut app, "ctrl+h");
    let buf = render(&mut app);
    assert!(find_on(&buf, "Replace").is_some(), "replace row is shown");
    typed(&mut app, "cat");
    press(&mut app, "tab");
    typed(&mut app, "bird");
    press(&mut app, "enter"); // replace current (first)
    assert_eq!(text_of(&app), "bird dog cat dog cat");
    press(&mut app, "alt+a"); // replace all remaining
    assert_eq!(text_of(&app), "bird dog bird dog bird");
    press(&mut app, "ctrl+z");
    assert_eq!(text_of(&app), "bird dog cat dog cat", "replace-all is a single undo step");
}

#[test]
fn find_bar_buttons_work_with_the_mouse() {
    let (mut app, _) = app_with("Foo foo FOO");
    press(&mut app, "ctrl+f");
    typed(&mut app, "foo");
    let buf = render(&mut app);
    let (x, y) = find_on(&buf, "Aa").unwrap();
    click(&mut app, x, y);
    assert!(screen(&mut app).join("\n").contains("1 of 1"), "Match-case button toggles");
    let (x, y) = find_on(&render(&mut app), "↓").unwrap();
    click(&mut app, x, y);
    assert!(screen(&mut app).join("\n").contains("1 of 1"));
    let (x, y) = find_on(&render(&mut app), "✕").unwrap();
    click(&mut app, x, y);
    assert!(find_on(&render(&mut app), "Find").is_none(), "close button closes");
}

#[test]
fn defaults_from_config_seed_the_toggles() {
    let mut cfg = Config::default();
    cfg.find.case_sensitive = true;
    let (mut app, _) = app_cfg("Foo foo", cfg);
    press(&mut app, "ctrl+f");
    typed(&mut app, "foo");
    assert!(screen(&mut app).join("\n").contains("1 of 1"));
}

// ---------------------------------------------------------------- mouse

#[test]
fn click_places_the_cursor() {
    let (mut app, _) = app_with(SAMPLE);
    let gw = gutter_w(&app);
    render(&mut app);
    click(&mut app, gw + 4, 1); // line 2, col 4 ("foo |bar")
    assert_eq!(app.buffer.cursor_line_col(), (1, 4));
    click(&mut app, 70, 2); // far right clamps to line end (normal mode: last char)
    assert_eq!(app.buffer.cursor_line_col().0, 2);
    click(&mut app, 9, 40 % 22);
}

#[test]
fn drag_selects_text() {
    let (mut app, _) = app_with(SAMPLE);
    let gw = gutter_w(&app);
    render(&mut app);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), gw, 0);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), gw + 5, 0);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), gw + 5, 0);
    assert_eq!(app.buffer.selected_text().as_deref(), Some("alpha"));
    let buf = render(&mut app);
    assert!(buf[(gw, 0)].modifier.contains(Modifier::REVERSED), "selection is reverse video");
    // multi-line drag
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), gw + 6, 0);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), gw + 3, 1);
    assert_eq!(app.buffer.selected_text().as_deref(), Some("beta\nfoo"));
}

#[test]
fn double_click_selects_a_word_and_triple_click_a_line() {
    let (mut app, _) = app_with(SAMPLE);
    let gw = gutter_w(&app);
    render(&mut app);
    click(&mut app, gw + 6, 0);
    click(&mut app, gw + 6, 0);
    assert_eq!(app.buffer.selected_text().as_deref(), Some("beta"));
    click(&mut app, gw + 6, 0);
    assert_eq!(app.buffer.selected_text().as_deref(), Some("alpha beta\n"), "third click selects the line");
}

#[test]
fn slow_second_click_is_not_a_double_click() {
    let mut cfg = Config::default();
    cfg.editor.double_click_ms = 1;
    let (mut app, _) = app_cfg(SAMPLE, cfg);
    render(&mut app);
    click(&mut app, 6, 0);
    std::thread::sleep(std::time::Duration::from_millis(30));
    click(&mut app, 6, 0);
    assert!(app.buffer.selection().is_none());
}

#[test]
fn clicking_the_gutter_selects_the_whole_line() {
    let (mut app, _) = app_with(SAMPLE);
    render(&mut app);
    click(&mut app, 1, 1);
    assert_eq!(app.buffer.selected_text().as_deref(), Some("foo bar foo\n"));
}

#[test]
fn wheel_scrolls_the_view_without_moving_the_cursor() {
    let text: String = (1..=100).map(|i| format!("line {i}\n")).collect();
    let (mut app, _) = app_with(&text);
    render(&mut app);
    mouse(&mut app, MouseEventKind::ScrollDown, 20, 5);
    let s = screen(&mut app);
    assert!(s[0].contains("line 4"), "scrolled by 3 lines: {:?}", s[0]);
    assert_eq!(app.buffer.cursor_line_col(), (0, 0), "cursor stays put");
    mouse(&mut app, MouseEventKind::ScrollUp, 20, 5);
    assert!(screen(&mut app)[0].contains("line 1"));
}

#[test]
fn mouse_works_in_insert_mode_and_modeless_mode_too() {
    let (mut app, _) = app_with(SAMPLE);
    let gw = gutter_w(&app);
    press(&mut app, "i");
    render(&mut app);
    click(&mut app, gw + 3, 0);
    assert_eq!(app.buffer.cursor_line_col(), (0, 3));
    typed(&mut app, "X");
    assert!(text_of(&app).starts_with("alpXha"));
    let (mut app, _) = modeless(SAMPLE);
    let gw = gutter_w(&app);
    render(&mut app);
    click(&mut app, gw + 2, 3);
    assert_eq!(app.buffer.cursor_line_col(), (3, 2));
}

#[test]
fn mouse_can_be_disabled() {
    let mut cfg = Config::default();
    cfg.editor.mouse = false;
    let (mut app, _) = app_cfg(SAMPLE, cfg);
    render(&mut app);
    click(&mut app, 9, 2);
    assert_eq!(app.buffer.cursor, 0);
}

// ---------------------------------------------------------------- clipboard

#[test]
fn ctrl_c_ctrl_x_ctrl_v_ctrl_a() {
    let (mut app, clip) = modeless("hello world");
    press(&mut app, "ctrl+a ctrl+c");
    assert_eq!(*clip.0.lock().unwrap(), "hello world");
    press(&mut app, "home");
    press(&mut app, "shift+right shift+right shift+right shift+right shift+right ctrl+x");
    assert_eq!(text_of(&app), " world");
    assert_eq!(*clip.0.lock().unwrap(), "hello");
    press(&mut app, "end ctrl+v");
    assert_eq!(text_of(&app), " worldhello");
    press(&mut app, "ctrl+a ctrl+v");
    assert_eq!(text_of(&app), "hello", "paste replaces the selection");
    press(&mut app, "ctrl+z");
    assert_eq!(text_of(&app), " worldhello");
}

#[test]
fn ctrl_c_without_selection_copies_the_line_and_paste_inserts_above() {
    let (mut app, clip) = modeless("one\ntwo\nthree");
    press(&mut app, "down ctrl+c");
    assert_eq!(*clip.0.lock().unwrap(), "two\n");
    press(&mut app, "down ctrl+v");
    assert_eq!(text_of(&app), "one\ntwo\ntwo\nthree");
}

#[test]
fn alt_c_x_v_are_alternatives_when_ctrl_is_reserved() {
    let (mut app, clip) = modeless("abc def");
    press(&mut app, "shift+right shift+right shift+right alt+c");
    assert_eq!(*clip.0.lock().unwrap(), "abc");
    press(&mut app, "alt+x");
    assert_eq!(text_of(&app), " def");
    press(&mut app, "end alt+v");
    assert_eq!(text_of(&app), " defabc");
}

#[test]
fn ctrl_c_works_in_normal_and_visual_mode() {
    let (mut app, clip) = app_with("hello world");
    press(&mut app, "v l l l l ctrl+c");
    assert_eq!(*clip.0.lock().unwrap(), "hello", "visual selection includes the char under the cursor");
    press(&mut app, "esc ctrl+a ctrl+c");
    assert_eq!(*clip.0.lock().unwrap(), "hello world");
}

#[test]
fn bracketed_paste_inserts_text() {
    let (mut app, _) = modeless("");
    app.handle_paste("line1\r\nline2");
    assert_eq!(text_of(&app), "line1\nline2");
}

// ---------------------------------------------------------------- usability

#[test]
fn undo_redo_save_and_quit_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("f.txt");
    std::fs::write(&p, "abc").unwrap();
    let (mut app, _) = modeless("");
    app.open_file(&p);
    typed(&mut app, "XY");
    assert_eq!(text_of(&app), "XYabc");
    press(&mut app, "ctrl+z");
    assert_eq!(text_of(&app), "abc");
    press(&mut app, "ctrl+y");
    assert_eq!(text_of(&app), "XYabc");
    press(&mut app, "ctrl+z ctrl+shift+z");
    assert_eq!(text_of(&app), "XYabc", "Ctrl+Shift+Z also redoes");

    press(&mut app, "ctrl+q");
    assert!(!app.quit, "unsaved changes: must ask first");
    let s = screen(&mut app);
    assert!(s[23].contains("Save changes to") && s[23].contains("f.txt"), "{:?}", s[23]);
    press(&mut app, "c");
    assert!(!app.quit);
    press(&mut app, "ctrl+s");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "XYabc");
    press(&mut app, "ctrl+q");
    assert!(app.quit, "clean buffer quits immediately");
}

#[test]
fn quit_prompt_yes_saves_and_no_discards() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("g.txt");
    std::fs::write(&p, "a").unwrap();
    let (mut app, _) = modeless("");
    app.open_file(&p);
    typed(&mut app, "b");
    press(&mut app, "ctrl+q y");
    assert!(app.quit);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "ba");
    let (mut app, _) = modeless("");
    app.open_file(&p);
    typed(&mut app, "z");
    press(&mut app, "ctrl+q n");
    assert!(app.quit);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "ba", "discarded");
}

#[test]
fn ctrl_s_without_a_name_asks_for_one() {
    let (mut app, _) = modeless("data");
    press(&mut app, "ctrl+s");
    assert!(screen(&mut app)[23].starts_with(":w"));
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("new.txt");
    typed(&mut app, &p.display().to_string());
    press(&mut app, "enter");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "data");
}

#[test]
fn modeless_config_gives_a_vscode_feel() {
    let (mut app, _) = modeless("");
    typed(&mut app, "hello\n  indented");
    press(&mut app, "enter");
    typed(&mut app, "x");
    assert_eq!(text_of(&app), "hello\n  indented\n  x", "auto-indent");
    assert!(!screen(&mut app)[22].contains("NORMAL"));
    press(&mut app, "esc");
    typed(&mut app, "y");
    assert!(text_of(&app).ends_with("xy"), "Esc does not switch to a command mode");
    press(&mut app, "tab");
    assert!(text_of(&app).ends_with("xy\t"), "Tab inserts a hard tab by default: {:?}", text_of(&app));
    press(&mut app, "home home");
    assert_eq!(app.buffer.cursor_line_col().1, 0);
}

#[test]
fn modal_editing_basics() {
    let (mut app, _) = app_with("one two three\nsecond line\nthird");
    assert!(screen(&mut app)[22].contains("NORMAL"));
    press(&mut app, "w x");
    assert_eq!(app.buffer.line(0), "one wo three");
    press(&mut app, "u");
    assert_eq!(app.buffer.line(0), "one two three");
    press(&mut app, "d w");
    assert_eq!(app.buffer.line(0), "one three");
    press(&mut app, "j d d");
    assert_eq!(text_of(&app), "one three\nthird");
    press(&mut app, "k y y p");
    assert_eq!(text_of(&app), "one three\none three\nthird");
    press(&mut app, "G o");
    typed(&mut app, "new");
    press(&mut app, "esc");
    assert_eq!(text_of(&app), "one three\none three\nthird\nnew");
    assert!(screen(&mut app)[22].contains("NORMAL"));
    press(&mut app, "g g D");
    assert_eq!(app.buffer.line(0), "");
    press(&mut app, "ctrl+r"); // redo nothing: must not panic
    press(&mut app, "u");
    assert_eq!(app.buffer.line(0), "one three");
}

#[test]
fn counts_and_line_motions() {
    let (mut app, _) = app_with("a\nb\nc\nd\ne");
    press(&mut app, "2 j");
    assert_eq!(app.buffer.cursor_line_col().0, 2);
    press(&mut app, "d 2 d");
    assert_eq!(text_of(&app), "a\nb\ne");
    press(&mut app, "g g 3 x");
    assert_eq!(text_of(&app), "\nb\ne");
    press(&mut app, "G");
    assert_eq!(app.buffer.cursor_line_col().0, 2);
    press(&mut app, "2 g g");
    assert_eq!(app.buffer.cursor_line_col().0, 1);
}

#[test]
fn visual_mode_select_delete_yank_paste() {
    let (mut app, _) = app_with("hello world foo");
    press(&mut app, "v e d");
    assert_eq!(text_of(&app), " world foo");
    press(&mut app, "w v e y");
    press(&mut app, "$ p");
    assert_eq!(text_of(&app), " world fooworld");
}

#[test]
fn visual_line_mode_and_change() {
    let (mut app, _) = app_with("a\nb\nc");
    press(&mut app, "V j d");
    assert_eq!(text_of(&app), "c");
    let (mut app, _) = app_with("hello there");
    press(&mut app, "c w");
    typed(&mut app, "bye");
    press(&mut app, "esc");
    assert_eq!(text_of(&app), "bye there");
}

#[test]
fn colon_commands() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("c.txt");
    let (mut app, _) = app_with("l1\nl2\nl3");
    press(&mut app, ":");
    typed(&mut app, "3");
    press(&mut app, "enter");
    assert_eq!(app.buffer.cursor_line_col().0, 2);
    press(&mut app, "x");
    press(&mut app, ":");
    typed(&mut app, &format!("w {}", p.display()));
    press(&mut app, "enter");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "l1\nl2\n3");
    press(&mut app, ":");
    typed(&mut app, "bogus");
    press(&mut app, "enter");
    assert!(screen(&mut app)[23].contains("Not an editor command"));
    press(&mut app, ": q enter");
    // ": q" typed with a space is the command "" then q... use the real thing:
    press(&mut app, ":");
    typed(&mut app, "q");
    press(&mut app, "enter");
    assert!(app.quit);
}

#[test]
fn ctrl_p_opens_the_command_line_in_any_mode() {
    let (mut app, _) = modeless("x");
    press(&mut app, "ctrl+p");
    assert!(screen(&mut app)[23].starts_with(':'));
    typed(&mut app, "q");
    press(&mut app, "enter");
    assert!(app.quit);
}

#[test]
fn tiny_terminals_do_not_panic() {
    let (mut app, _) = app_with(SAMPLE);
    press(&mut app, "ctrl+f");
    typed(&mut app, "foo");
    for (w, h) in [(1, 1), (5, 3), (20, 4), (30, 8), (200, 60)] {
        render_size(&mut app, w, h);
    }
    // every small size, with the find bar (and replace row) open
    press(&mut app, "ctrl+h");
    for w in 0..45 {
        for h in 0..14 {
            render_size(&mut app, w, h);
        }
    }
    common::sqlview::show_rows(&mut app, 10, 5);
    for (w, h) in [(10, 5), (40, 9), (80, 12)] {
        render_size(&mut app, w, h);
    }
}

#[test]
fn control_characters_in_the_buffer_cannot_escape_into_the_terminal() {
    let (mut app, _) = app_with("a\x1b[31mred\x07b");
    let s = screen(&mut app);
    assert!(!s[0].contains('\x1b') && !s[0].contains('\x07'), "{:?}", s[0]);
}

#[test]
fn wide_and_tab_characters_keep_the_cursor_aligned() {
    let (mut app, _) = modeless("日本語\n\tx");
    let gw = gutter_w(&app);
    press(&mut app, "right right");
    let area = ratatui::layout::Rect::new(0, 0, W, H);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    let cur = app.render(&mut buf, area).unwrap();
    assert_eq!(cur, (gw + 4, 0), "two double-width chars");
    press(&mut app, "down home");
    let cur = app.render(&mut buf, area).unwrap();
    assert_eq!(cur.0, gw + 8, "after home on '\\tx' the cursor is on the first non-blank (after the 8-wide tab)");
}

#[test]
fn horizontal_scroll_follows_the_cursor_on_long_lines() {
    let long = "x".repeat(300);
    let (mut app, _) = modeless(&long);
    press(&mut app, "end");
    let area = ratatui::layout::Rect::new(0, 0, W, H);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    let cur = app.render(&mut buf, area).unwrap();
    assert!(cur.0 < W);
    assert!(app.left_col > 200);
}

#[test]
fn mods_are_ignored_for_keyboard_release_events() {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
    let (mut app, _) = modeless("");
    let mut e = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
    e.kind = KeyEventKind::Release;
    app.handle_key(e);
    assert_eq!(text_of(&app), "");
}

// ---------------------------------------------------------------- regression: line numbers while editing

/// First whitespace-separated token of each text row of the gutter, for rows `0..n`.
fn gutter_numbers(app: &mut kuro::app::App, n: usize) -> Vec<String> {
    screen(app).iter().take(n).map(|r| r.split_whitespace().next().unwrap_or("").to_string()).collect()
}

fn ten_lines() -> String {
    (1..=10).map(|i| format!("line number {i}")).collect::<Vec<_>>().join("\n")
}

fn absolute() -> Vec<String> {
    (1..=10).map(|i| i.to_string()).collect()
}

/// REGRESSION: in a 10-line file, editing line 5 made the numbers above and below it restart
/// from 1 (4 3 2 1 5 1 2 3 4 5) instead of staying absolute (1..10).
/// Root cause: `editor.relative_numbers` defaulted to true, so every line other than the cursor line
/// was rendered as its distance from the cursor line. Not a stale line count: the gutter always
/// used the current count. Fix: absolute numbers are the default; relative is opt-in.
#[test]
fn regression_gutter_stays_absolute_while_editing_line_5_of_10() {
    let (mut app, _) = app_with(&ten_lines());
    assert_eq!(gutter_numbers(&mut app, 10), absolute(), "before any edit");
    press(&mut app, "4 j"); // cursor on line 5
    assert_eq!(app.buffer.cursor_line_col().0, 4);
    assert_eq!(gutter_numbers(&mut app, 10), absolute(), "cursor on line 5, normal mode");
    press(&mut app, "A");
    typed(&mut app, " edited");
    assert_eq!(gutter_numbers(&mut app, 10), absolute(), "while typing on line 5");
    press(&mut app, "esc");
    assert_eq!(gutter_numbers(&mut app, 10), absolute(), "after leaving insert mode");
    press(&mut app, "o");
    typed(&mut app, "new line");
    let mut want = absolute();
    want.push("11".into());
    assert_eq!(gutter_numbers(&mut app, 11), want, "a line added after line 5 renumbers the rest, still absolute");
    press(&mut app, "esc d d");
    assert_eq!(gutter_numbers(&mut app, 10), absolute(), "after deleting a line");
}

#[test]
fn regression_gutter_stays_absolute_when_modeless() {
    let (mut app, _) = modeless(&ten_lines());
    press(&mut app, "down down down down");
    typed(&mut app, "x");
    assert_eq!(gutter_numbers(&mut app, 10), absolute());
}
