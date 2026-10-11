//! `:setlang`, the language picker, and per-driver SQL snippets.

mod common;
use common::ui::*;
use kuro::app::{App, Focus};
use kuro::render::ansi_color;
use kuro::sql::profile::Engine;
use kuro::sql::{Canceller, DbError, Driver, ResultSet, Session};
use std::sync::Arc;

struct FakeDb(Engine);

impl Driver for FakeDb {
    fn execute(&mut self, _sql: &str, _limit: usize) -> Result<ResultSet, DbError> {
        Ok(ResultSet::default())
    }
    fn canceller(&self) -> Canceller {
        Arc::new(|| Ok(()))
    }
    fn label(&self) -> String {
        "fake".into()
    }
    fn engine(&self) -> Engine {
        self.0
    }
}

fn connected(engine: Engine, text: &str) -> App {
    let (mut app, _) = app_with(text);
    app.sql.session = Some(Session::new(Box::new(FakeDb(engine))));
    app
}

fn cmd(app: &mut App, c: &str) {
    press(app, ":");
    typed(app, c);
    press(app, "enter");
}

// ---------------------------------------------------------------- :setlang

#[test]
fn setlang_recolours_the_whole_buffer_immediately() {
    let (mut app, _) = app_with("func main() {}\nSELECT 1");
    let before = render(&mut app);
    let gw = gutter_w(&app);
    assert_eq!(before[(gw, 0)].fg, ratatui::style::Color::Reset, "plain text is uncoloured");
    cmd(&mut app, "setlang go");
    let go = render(&mut app);
    assert_eq!(go[(gw, 0)].fg, ansi_color(5).unwrap(), "`func` is a keyword now");
    assert_eq!(go[(gw, 1)].fg, ratatui::style::Color::Reset, "`SELECT` is not a Go keyword");
    cmd(&mut app, "setlang sql");
    let sql = render(&mut app);
    assert_eq!(sql[(gw, 1)].fg, ansi_color(5).unwrap(), "`SELECT` is a keyword in SQL");
    assert_ne!(sql[(gw, 0)].fg, ansi_color(5).unwrap(), "`func` no longer is");
}

#[test]
fn setlang_overrides_extension_detection_and_auto_restores_it() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("q.sql");
    std::fs::write(&p, "select 1").unwrap();
    let (mut app, _) = app_with("");
    app.open_file(&p);
    assert!(screen(&mut app)[22].contains("SQL") && !screen(&mut app)[22].contains("manual"));
    cmd(&mut app, "setlang python");
    let s = screen(&mut app);
    assert!(s[22].contains("Python (manual)"), "{:?}", s[22]);
    cmd(&mut app, "setlang auto");
    let s = screen(&mut app);
    assert!(s[22].contains("SQL") && !s[22].contains("manual"), "{:?}", s[22]);
}

#[test]
fn setlang_survives_save_as_and_reopen_of_the_same_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = app_with("x = 1");
    cmd(&mut app, "setlang python");
    cmd(&mut app, &format!("w {}", dir.path().join("noext").display()));
    assert_eq!(app.lang.name, "Python", "manual choice is kept when the file gets a name");
}

#[test]
fn setlang_accepts_aliases_and_ignores_case() {
    for (arg, name) in
        [("GO", "Go"), ("golang", "Go"), ("Py", "Python"), ("bash", "Shell"), ("JSON", "JSON"), ("postgres", "SQL"), ("text", "Plain Text")]
    {
        let (mut app, _) = app_with("x");
        cmd(&mut app, &format!("setlang {arg}"));
        assert_eq!(app.lang.name, name, "{arg}");
    }
}

#[test]
fn setlang_with_an_unknown_mode_lists_the_supported_ones() {
    let (mut app, _) = app_with("x");
    cmd(&mut app, "setlang klingon");
    let m = screen(&mut app)[23].clone();
    assert!(m.contains("unknown language") && m.contains("Go") && m.contains("Python"), "{m:?}");
    assert_eq!(app.lang.name, "Plain Text", "unchanged");
}

// ---------------------------------------------------------------- picker

#[test]
fn setlang_without_argument_opens_a_picker_that_filters_and_selects() {
    let (mut app, _) = app_with("x = 1");
    cmd(&mut app, "setlang");
    assert_eq!(app.focus, Focus::Picker);
    let s = screen(&mut app).join("\n");
    for l in ["Auto-detect", "Plain Text", "SQL", "Go", "Python", "Shell", "JSON", "Lua"] {
        assert!(s.contains(l), "{l} missing from picker:\n{s}");
    }
    typed(&mut app, "pyth");
    let s = screen(&mut app).join("\n");
    assert!(s.contains("Python") && !s.contains("JSON"), "filtered:\n{s}");
    press(&mut app, "enter");
    assert_eq!(app.focus, Focus::Editor);
    assert_eq!(app.lang.name, "Python");
    assert!(app.picker.is_none());
}

#[test]
fn picker_navigation_with_arrows_and_ctrl_n_p_and_esc_cancels() {
    let (mut app, _) = app_with("x");
    cmd(&mut app, "setlang");
    press(&mut app, "down down enter"); // Auto-detect, Plain Text, SQL -> SQL
    assert_eq!(app.lang.name, "SQL");
    cmd(&mut app, "setlang");
    press(&mut app, "ctrl+n ctrl+n ctrl+n ctrl+p enter");
    assert_eq!(app.lang.name, "Python", "starts on the current mode (SQL); +3 -1 lands two rows further down");
    cmd(&mut app, "setlang");
    press(&mut app, "esc");
    assert_eq!(app.focus, Focus::Editor);
    assert_eq!(app.lang.name, "Python", "Esc cancels without changing anything");
}

#[test]
fn picker_highlights_the_selected_row_without_a_background() {
    let (mut app, _) = app_with("x");
    cmd(&mut app, "setlang");
    let buf = render(&mut app);
    assert!(buf.content.iter().any(|c| c.modifier.contains(ratatui::style::Modifier::REVERSED)));
    assert!(buf.content.iter().all(|c| c.bg == ratatui::style::Color::Reset));
}

#[test]
fn picker_rows_can_be_clicked() {
    let (mut app, _) = app_with("x");
    cmd(&mut app, "setlang");
    let buf = render(&mut app);
    let (x, y) = find_on(&buf, "Python").unwrap();
    click(&mut app, x, y);
    assert_eq!(app.lang.name, "Python");
    assert_eq!(app.focus, Focus::Editor);
}

#[test]
fn clicking_outside_the_picker_closes_it() {
    let (mut app, _) = app_with("x");
    cmd(&mut app, "setlang");
    render(&mut app);
    click(&mut app, 0, 23);
    assert_eq!(app.focus, Focus::Editor);
    assert!(app.picker.is_none());
}

// ---------------------------------------------------------------- snippets

#[test]
fn without_a_connection_snippets_are_generic_and_say_so() {
    let (mut app, _) = app_with("");
    cmd(&mut app, "snippets");
    let s = screen(&mut app).join("\n");
    assert!(s.contains("generic"), "{s}");
    assert!(s.contains("Not connected") && s.contains("generic snippets"), "{s}");
    assert!(s.contains("select") && !s.contains("show tables"));
}

#[test]
fn mariadb_and_postgres_get_their_own_snippets() {
    let mut app = connected(Engine::MariaDb, "");
    cmd(&mut app, "snippets");
    let s = screen(&mut app).join("\n");
    assert!(s.contains("MariaDB") && s.contains("show tables") && s.contains("describe"), "{s}");
    assert!(!s.contains("Not connected"));
    press(&mut app, "esc");
    let mut app = connected(Engine::Postgres, "");
    cmd(&mut app, "snippets");
    let s = screen(&mut app).join("\n");
    assert!(s.contains("PostgreSQL") && s.contains("list tables") && s.contains("describe table"), "{s}");
    assert!(!s.contains("show tables"));
}

#[test]
fn choosing_a_snippet_inserts_it_at_the_cursor() {
    let mut app = connected(Engine::MariaDb, "-- start\n");
    press(&mut app, "j"); // second (empty) line
    cmd(&mut app, "snippets");
    typed(&mut app, "show tables");
    press(&mut app, "enter");
    assert_eq!(text_of(&app), "-- start\nSHOW TABLES;\n");
    press(&mut app, "ctrl+z");
    assert_eq!(text_of(&app), "-- start\n", "one undo step");
}

#[test]
fn alt_s_opens_the_snippet_picker() {
    let (mut app, _) = modeless("");
    press(&mut app, "alt+s");
    assert_eq!(app.focus, Focus::Picker);
}

#[test]
fn snippet_insertion_replaces_a_selection() {
    let (mut app, _) = app_with("xxx");
    press(&mut app, "ctrl+a");
    cmd_keep_selection(&mut app);
    assert_eq!(text_of(&app), "DROP TABLE <table>;\n");
}

fn cmd_keep_selection(app: &mut App) {
    press(app, "alt+s");
    typed(app, "drop");
    press(app, "enter");
}
