//! SQL result table and query flow through the UI (no database: a fake driver).

mod common;
use common::sqlview::*;
use common::ui::*;
use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
use kuro::app::{App, Focus};
use kuro::config::Config;
use kuro::sql::{Canceller, DbError, Driver, ErrorCategory, ResultSet, Session};
use kuro::table::{Column, display_width};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Row indices of the pane: find the header row by the column name.
fn header_row(s: &[String]) -> usize {
    s.iter().position(|r| r.contains("col0")).expect("header row")
}

#[test]
fn table_shows_header_with_types_and_at_most_max_rows() {
    let (mut app, _) = app_with("select 1");
    show_rows(&mut app, 100, 3);
    let s = screen(&mut app);
    let h = header_row(&s);
    assert!(s[h].contains("col0 text") && s[h].contains("col1 text"), "{:?}", s[h]);
    assert!(s[h + 1].contains('┼') || s[h + 1].contains('─'), "separator under the header");
    let body: Vec<&String> = s[h + 2..].iter().take_while(|r| r.starts_with('r')).collect();
    assert!(body.len() <= 25, "max 25 rows visible by default, got {}", body.len());
    assert!(body.len() >= 10, "and a reasonable number fit on a 24-row screen: {}", body.len());
    assert!(s.iter().any(|r| r.contains("100 rows") && r.contains("ms")), "status line has row count and timing");
    // the editor is still visible above
    assert!(s[0].contains("select 1"));
}

#[test]
fn row_cap_is_configurable() {
    let mut cfg = Config::default();
    cfg.sql.table_max_rows = 4;
    let (mut app, _) = app_cfg("x", cfg);
    show_rows(&mut app, 50, 2);
    let s = screen(&mut app);
    let h = header_row(&s);
    assert_eq!(s[h + 2..].iter().take_while(|r| r.starts_with('r')).count(), 4);
}

#[test]
fn vertical_scroll_by_keyboard_keeps_the_header_visible() {
    let (mut app, _) = app_with("x");
    show_rows(&mut app, 100, 2);
    press(&mut app, "f4");
    assert_eq!(app.focus, Focus::Table);
    let first = |app: &mut App| {
        let s = screen(app);
        let h = header_row(&s);
        (s[h].clone(), s[h + 2].clone())
    };
    let (head0, row0) = first(&mut app);
    assert!(row0.starts_with("r0c0"));
    press(&mut app, "j j j");
    let (head1, row1) = first(&mut app);
    assert!(row1.starts_with("r3c0"), "{row1:?}");
    assert_eq!(head0, head1, "header does not scroll away");
    press(&mut app, "pagedown");
    assert!(first(&mut app).1.starts_with("r"), "page down moves");
    press(&mut app, "G");
    let s = screen(&mut app);
    assert!(s.iter().any(|r| r.starts_with("r99c0")), "G shows the last row");
    assert!(s.iter().any(|r| r.contains("rows ") && r.contains("/100")), "scroll position is shown");
    press(&mut app, "g");
    assert!(first(&mut app).1.starts_with("r0c0"));
    press(&mut app, "k"); // already at the top: stays clamped
    assert!(first(&mut app).1.starts_with("r0c0"));
}

#[test]
fn vertical_scroll_by_mouse_wheel_over_the_table() {
    let (mut app, _) = app_with("x");
    show_rows(&mut app, 100, 2);
    let s = screen(&mut app);
    let y = (header_row(&s) + 3) as u16;
    mouse(&mut app, MouseEventKind::ScrollDown, 4, y);
    let s = screen(&mut app);
    let h = header_row(&s);
    assert!(s[h + 2].starts_with("r3c0"), "{:?}", s[h + 2]);
    mouse(&mut app, MouseEventKind::ScrollUp, 4, y);
    assert!(screen(&mut app)[h + 2].starts_with("r0c0"));
    // the wheel over the *editor* must not scroll the table
    mouse(&mut app, MouseEventKind::ScrollDown, 4, 0);
    assert!(screen(&mut app)[h + 2].starts_with("r0c0"));
}

#[test]
fn horizontal_scroll_moves_header_and_rows_together() {
    let (mut app, _) = app_with("x");
    show_rows(&mut app, 5, 12);
    let s = screen(&mut app);
    let h = header_row(&s);
    assert!(!s[h].contains("col11"), "does not all fit");
    press(&mut app, "f4 end");
    let s = screen(&mut app);
    let h = s.iter().position(|r| r.contains("col11")).expect("scrolled to last column");
    assert!(!s[h].contains("col0 "), "scrolled past the first column: {:?}", s[h]);
    assert!(s[h + 2].contains("r0c11"), "body follows the header: {:?}", s[h + 2]);
    press(&mut app, "home");
    assert!(screen(&mut app)[h].starts_with("col0"));
    press(&mut app, "ctrl+right");
    assert!(!screen(&mut app)[h].starts_with("col0"), "Ctrl+Right scrolls by cells");
    // mouse: shift+wheel and horizontal wheel
    press(&mut app, "home");
    mouse_mod(&mut app, MouseEventKind::ScrollDown, 4, (h + 3) as u16, KeyModifiers::SHIFT);
    assert!(!screen(&mut app)[h].starts_with("col0"));
    press(&mut app, "home");
    mouse(&mut app, MouseEventKind::ScrollRight, 4, (h + 3) as u16);
    assert!(!screen(&mut app)[h].starts_with("col0"));
}

#[test]
fn wide_columns_are_truncated_with_an_ellipsis_and_expand_on_demand() {
    let (mut app, _) = app_with("x");
    let rs = ResultSet {
        columns: vec![Column { name: "a".into(), type_name: "text".into() }, Column { name: "b".into(), type_name: "int".into() }],
        rows: vec![vec!["y".repeat(100), "7".into()]],
        total_rows: 1,
        ..Default::default()
    };
    show(&mut app, rs);
    let s = screen(&mut app);
    let row = s.iter().find(|r| r.starts_with("yyy")).unwrap().clone();
    assert!(row.contains('…'), "{row:?}");
    assert!(!row.contains(&"y".repeat(60)), "truncated at column_max_width (40)");
    assert!(row.contains(" 7"), "the next column is still visible");
    press(&mut app, "f4 e");
    let s = screen(&mut app);
    let row = s.iter().find(|r| r.starts_with("yyy")).unwrap().clone();
    assert!(row.contains(&"y".repeat(79)), "expanded shows the full 100 chars (bounded by the viewport): {row:?}");
    press(&mut app, "e");
    let s = screen(&mut app);
    assert!(s.iter().find(|r| r.starts_with("yyy")).unwrap().contains('…'), "e again collapses");
}

#[test]
fn table_never_exceeds_the_configured_max_width() {
    let mut cfg = Config::default();
    cfg.sql.table_max_width = 30;
    let (mut app, _) = app_cfg("x", cfg);
    show_rows(&mut app, 3, 8);
    let s = screen(&mut app);
    let h = header_row(&s);
    for r in &s[h..h + 5] {
        let used = r.trim_end();
        assert!(display_width(used) <= 30, "{used:?} is wider than table_max_width=30");
    }
    // ...and scrolling reveals the rest
    press(&mut app, "f4 end");
    assert!(screen(&mut app).iter().any(|r| r.contains("col7")));
}

#[test]
fn wide_unicode_cells_stay_aligned() {
    let (mut app, _) = app_with("x");
    let rs = ResultSet {
        columns: vec![Column { name: "col0".into(), type_name: "".into() }, Column { name: "col1".into(), type_name: "".into() }],
        rows: vec![vec!["日本語テキスト".into(), "a".into()], vec!["x".into(), "b".into()]],
        total_rows: 2,
        ..Default::default()
    };
    show(&mut app, rs);
    let s = screen(&mut app);
    let h = header_row(&s);
    let pos = |r: &str| display_width(&r[..r.find('│').unwrap()]);
    assert_eq!(pos(&s[h]), pos(&s[h + 2]));
    assert_eq!(pos(&s[h + 2]), pos(&s[h + 3]), "column separators line up");
}

#[test]
fn clicking_a_header_selects_the_column_and_double_click_expands_it() {
    let (mut app, _) = app_with("x");
    let rs = ResultSet {
        columns: vec![Column { name: "col0".into(), type_name: "".into() }, Column { name: "col1".into(), type_name: "".into() }],
        rows: vec![vec!["a".repeat(60), "b".repeat(60)]],
        total_rows: 1,
        ..Default::default()
    };
    show(&mut app, rs);
    let s = screen(&mut app);
    let h = header_row(&s) as u16;
    click(&mut app, 45, h); // inside the second column (first is 40 wide)
    assert_eq!(app.focus, Focus::Table);
    assert_eq!(app.sql.result.as_ref().unwrap().state.cur_col, 1);
    click(&mut app, 45, h);
    assert!(app.sql.result.as_ref().unwrap().state.expanded.contains(&1), "double click on a header expands");
}

#[test]
fn empty_result_set_shows_headers_and_zero_rows() {
    let (mut app, _) = app_with("x");
    show(
        &mut app,
        ResultSet { columns: vec![Column { name: "col0".into(), type_name: "int".into() }], total_rows: 0, ..Default::default() },
    );
    let s = screen(&mut app);
    assert!(s.iter().any(|r| r.contains("col0 int")));
    assert!(s.iter().any(|r| r.contains("0 rows")));
}

#[test]
fn errors_are_shown_in_the_pane_with_category_position_and_message() {
    let (mut app, _) = app_with("SELEC 1");
    show_error(&mut app, "syntax", "syntax error at or near \"SELEC\"", Some(1), Some(1));
    let s = screen(&mut app).join("\n");
    assert!(s.contains("syntax error at line 1, col 1"), "{s}");
    assert!(s.contains("syntax error at or near \"SELEC\""));
    assert!(s.contains("✖ syntax error"), "status line carries the category");
    assert!(s.contains("3.0 ms"), "and the timing: {s}");
    assert!(s.contains("SELEC 1"), "the editor stays visible");
    // and still works
    press(&mut app, "i");
    typed(&mut app, "!");
    assert!(text_of(&app).starts_with('!'));

    for (cat, label) in [("runtime", "runtime error"), ("execution", "execution error")] {
        show_error(&mut app, cat, "boom", None, None);
        assert!(screen(&mut app).join("\n").contains(label));
    }
}

#[test]
fn long_error_messages_wrap_inside_the_pane() {
    let (mut app, _) = app_with("x");
    let msg = "word ".repeat(60);
    show_error(&mut app, "execution", &msg, None, None);
    let s = screen(&mut app);
    for r in &s {
        assert!(display_width(r) <= 80);
    }
    assert!(s.iter().filter(|r| r.contains("word word")).count() >= 2, "wrapped over several lines");
}

#[test]
fn pane_leaves_room_for_the_editor_on_small_screens() {
    let (mut app, _) = app_with("a\nb\nc\nd\ne\nf");
    show_rows(&mut app, 100, 2);
    let l = app.layout(ratatui::layout::Rect::new(0, 0, 80, 12));
    assert!(l.text.height >= 4, "text area {}", l.text.height);
    assert!(l.pane.is_some());
    let l = app.layout(ratatui::layout::Rect::new(0, 0, 80, 7));
    assert!(l.pane.is_none(), "too small for a pane: hide it instead of squashing the editor");
}

#[test]
fn copy_table_as_tsv() {
    let (mut app, clip) = app_with("x");
    show_rows(&mut app, 2, 2);
    press(&mut app, "f4 ctrl+c");
    let t = clip.0.lock().unwrap().clone();
    assert!(t.starts_with("col0\tcol1\nr0c0-\tr0c1-xxx"), "{t:?}");
}

// ------------------------------------------------------------- fake driver

struct Fake {
    cancel: Arc<AtomicBool>,
}

impl Driver for Fake {
    fn execute(&mut self, sql: &str, _limit: usize) -> Result<ResultSet, DbError> {
        if sql.contains("SLOW") {
            let t = Instant::now();
            while !self.cancel.swap(false, Ordering::SeqCst) {
                if t.elapsed() > Duration::from_secs(10) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            return Err(DbError {
                category: ErrorCategory::Execution,
                message: "canceling statement due to user request".into(),
                line: None,
                column: None,
                code: Some("57014".into()),
            });
        }
        if sql.contains("BAD") {
            let col = sql.find("BAD").unwrap() + 1;
            let line = sql[..sql.find("BAD").unwrap()].matches('\n').count() + 1;
            let col = if line == 1 { col } else { col - sql.rfind('\n').unwrap() - 1 };
            return Err(DbError {
                category: ErrorCategory::Syntax,
                message: "syntax error at or near \"BAD\"".into(),
                line: Some(line),
                column: Some(col),
                code: None,
            });
        }
        let mut rs = rows_result(3, 2);
        rs.rows[0][0] = sql.to_string();
        Ok(rs)
    }
    fn canceller(&self) -> Canceller {
        let c = self.cancel.clone();
        Arc::new(move || {
            c.store(true, Ordering::SeqCst);
            Ok(())
        })
    }
    fn label(&self) -> String {
        "fake://db".into()
    }
}

fn connected(text: &str) -> App {
    let (mut app, _) = app_with(text);
    app.sql.session = Some(Session::new(Box::new(Fake { cancel: Arc::new(AtomicBool::new(false)) })));
    app
}

fn wait_for_result(app: &mut App) {
    let t = Instant::now();
    while app.sql.session.as_ref().is_some_and(|s| s.is_running()) {
        assert!(t.elapsed() < Duration::from_secs(10), "query never finished");
        std::thread::sleep(Duration::from_millis(5));
        app.tick();
    }
}

#[test]
fn f5_runs_the_selection_and_f6_runs_the_whole_buffer() {
    let mut app = connected("first line\nselect 42\nlast");
    press(&mut app, "j V");
    press(&mut app, "f5");
    wait_for_result(&mut app);
    let rs = app.sql.result.as_ref().unwrap().outcome.as_ref().unwrap();
    assert_eq!(rs.rows[0][0], "select 42\n", "only the selected line ran");
    press(&mut app, "f6");
    wait_for_result(&mut app);
    let rs = app.sql.result.as_ref().unwrap().outcome.as_ref().unwrap();
    assert_eq!(rs.rows[0][0], "first line\nselect 42\nlast");
    let s = screen(&mut app).join("\n");
    assert!(s.contains("3 rows"), "{s}");
    assert!(s.contains("fake://db"), "connection shown in the status bar");
}

#[test]
fn f5_without_a_selection_explains_itself() {
    let mut app = connected("select 1");
    press(&mut app, "f5");
    assert!(screen(&mut app)[23].contains("no selection"));
    assert!(app.sql.result.is_none());
}

#[test]
fn running_without_a_connection_explains_itself() {
    let (mut app, _) = app_with("select 1");
    press(&mut app, "f6");
    assert!(screen(&mut app)[23].contains("not connected"));
}

#[test]
fn error_line_and_column_are_mapped_back_to_the_buffer() {
    // the selection starts on buffer line 3 (index 2): "  BAD" is on the selection's first line
    let mut app = connected("one\ntwo\nok BAD stuff\nfour");
    press(&mut app, "j j V");
    press(&mut app, "f5");
    wait_for_result(&mut app);
    let e = app.sql.result.as_ref().unwrap().outcome.as_ref().unwrap_err();
    assert_eq!((e.line, e.column), (Some(3), Some(4)), "line 3 of the buffer, col 4");
    assert!(screen(&mut app).join("\n").contains("at line 3, col 4"));
}

#[test]
fn running_query_shows_progress_and_can_be_cancelled() {
    let mut app = connected("SLOW");
    press(&mut app, "f6");
    std::thread::sleep(Duration::from_millis(60));
    app.tick();
    assert!(app.sql.session.as_ref().unwrap().is_running());
    let s = screen(&mut app).join("\n");
    assert!(s.contains("running"), "{s}");
    press(&mut app, "f6");
    assert!(screen(&mut app)[23].contains("already running"), "second query is refused while one runs");
    let t = Instant::now();
    press(&mut app, "f7");
    wait_for_result(&mut app);
    assert!(t.elapsed() < Duration::from_secs(5), "cancel stops it promptly");
    let e = app.sql.result.as_ref().unwrap().outcome.as_ref().unwrap_err();
    assert_eq!(e.category, ErrorCategory::Execution);
    assert!(screen(&mut app).join("\n").contains("canceling statement"));
    // and the next query works
    app.buffer = kuro::buffer::Buffer::from_str("select 1");
    press(&mut app, "ctrl+a f5");
    wait_for_result(&mut app);
    assert!(app.sql.result.as_ref().unwrap().outcome.is_ok());
}

#[test]
fn colon_sql_commands() {
    let mut app = connected("select 1");
    press(&mut app, ":");
    typed(&mut app, "sql buffer");
    press(&mut app, "enter");
    wait_for_result(&mut app);
    assert!(app.sql.result.as_ref().unwrap().outcome.is_ok());
    press(&mut app, ":");
    typed(&mut app, "sql close");
    press(&mut app, "enter");
    assert!(!app.sql.pane_open);
    press(&mut app, ":");
    typed(&mut app, "sql cancel");
    press(&mut app, "enter");
    assert!(screen(&mut app)[23].contains("no query is running"));
    press(&mut app, ":");
    typed(&mut app, "sql disconnect");
    press(&mut app, "enter");
    assert!(app.sql.session.is_none());
    press(&mut app, ":");
    typed(&mut app, "sql connect nope");
    press(&mut app, "enter");
    let m = screen(&mut app)[23].clone();
    assert!(m.contains("no connection profiles") || m.contains("no profile"), "{m}");
}

#[test]
fn connect_reads_profiles_and_prompts_for_the_password_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("connections.toml");
    std::fs::write(&f, "[profiles.p]\ndriver = \"postgres\"\nhost = \"127.0.0.1\"\nport = 1\nuser = \"u\"\npassword = \"prompt\"\n")
        .unwrap();
    let mut cfg = Config::default();
    cfg.sql.connections_file = f.display().to_string();
    let (mut app, _) = app_cfg("x", cfg);
    press(&mut app, ":");
    typed(&mut app, "sql connect p");
    press(&mut app, "enter");
    typed(&mut app, "hunter2");
    let line = screen(&mut app)[23].clone();
    assert!(line.contains("password for p"), "{line:?}");
    assert!(!line.contains("hunter2"), "the password is masked on screen");
    assert!(line.contains("•••••••"));
    press(&mut app, "enter");
    // port 1 refuses the connection: reported as an execution error, never a crash
    let t = Instant::now();
    while app.sql.result.is_none() {
        assert!(t.elapsed() < Duration::from_secs(15));
        std::thread::sleep(Duration::from_millis(20));
        app.tick();
    }
    let e = app.sql.result.as_ref().unwrap().outcome.as_ref().unwrap_err();
    assert_eq!(e.category, ErrorCategory::Execution);
}

#[test]
fn unused_mouse_import_guard() {
    let _ = (MouseButton::Left, KeyModifiers::NONE);
}

#[test]
fn result_view_tab_shows_results_full_screen_and_esc_returns_to_the_editor() {
    let mut cfg = Config::default();
    cfg.sql.result_view = "tab".into();
    let (mut app, _) = app_cfg("select 1", cfg);
    app.sql.session = Some(Session::new(Box::new(Fake { cancel: Arc::new(AtomicBool::new(false)) })));
    press(&mut app, "f6");
    wait_for_result(&mut app);
    let s = screen(&mut app);
    assert_eq!(app.focus, Focus::Table, "results take the front");
    assert!(s[0].contains("col0"), "table starts on the first row: {:?}", s[0]);
    assert!(!s.iter().any(|r| r.contains("select 1") && r.contains("1 ") && r.starts_with("   1")), "editor is hidden");
    assert!(s.iter().any(|r| r.contains("3 rows")));
    press(&mut app, "esc");
    let s = screen(&mut app);
    assert_eq!(app.focus, Focus::Editor);
    assert!(s[0].contains("select 1"), "back in the editor");
    assert!(s[22].contains("F4 results"), "status bar says how to return: {:?}", s[22]);
    press(&mut app, "f4");
    assert_eq!(app.focus, Focus::Table);
    assert!(screen(&mut app)[0].contains("col0"));
}
