//! SQL behaviour (Part 4): disconnected state, Ctrl+Enter / fallback key / run-line, statement detection,
//! kitty-protocol notice, and the `:connect` family of commands. A recording fake driver stands in for the database.

mod common;
use common::ui::*;
use kuro::app::{App, DISCONNECTED_MSG, Focus};
use kuro::config::Config;
use kuro::sql::profile::Engine;
use kuro::sql::{Canceller, DbError, Driver, ResultSet, Session};
use kuro::syntax;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Recorder {
    engine: Engine,
    log: Arc<Mutex<Vec<String>>>,
}

impl Driver for Recorder {
    fn execute(&mut self, sql: &str, _limit: usize) -> Result<ResultSet, DbError> {
        self.log.lock().unwrap().push(sql.to_string());
        Ok(ResultSet { affected: Some(1), ..Default::default() })
    }
    fn canceller(&self) -> Canceller {
        Arc::new(|| Ok(()))
    }
    fn label(&self) -> String {
        "fake".into()
    }
    fn engine(&self) -> Engine {
        self.engine
    }
}

type Log = Arc<Mutex<Vec<String>>>;

fn sql_app(text: &str, cfg: Config) -> App {
    let (mut app, _) = app_cfg(text, cfg);
    app.set_lang(Some(&syntax::SQL));
    app
}

fn connect_fake(app: &mut App, engine: Engine) -> Log {
    let log: Log = Arc::default();
    app.sql.session = Some(Session::new(Box::new(Recorder { engine, log: log.clone() })));
    log
}

fn wait_idle(app: &mut App) {
    let t = Instant::now();
    while app.sql.session.as_ref().is_some_and(|s| s.is_running()) {
        assert!(t.elapsed() < Duration::from_secs(10), "query never finished");
        std::thread::yield_now();
        app.tick();
    }
}

fn ran(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

fn cmd(app: &mut App, c: &str) {
    press(app, ":");
    typed(app, c);
    press(app, "enter");
}

/// Put the cursor at (line, col), 0-based.
fn at(app: &mut App, line: usize, col: usize) {
    app.buffer.anchor = None;
    app.buffer.cursor = app.buffer.pos_of(line, col);
}

// ---------------------------------------------------------------- disconnected state

#[test]
fn disconnected_sql_buffer_stays_in_normal_mode_and_says_why() {
    let mut app = sql_app("select 1;", Config::default());
    let s = screen(&mut app);
    assert_eq!(s[23].trim_end(), DISCONNECTED_MSG, "the message is always visible");
    assert!(s[22].contains("NORMAL"));
    press(&mut app, "i");
    assert_eq!(app.mode_label(), "NORMAL", "Insert mode is blocked");
    assert!(screen(&mut app)[23].contains("Not connected to a database. Use :connect <profile>"));
    for k in ["a", "A", "I", "o", "O"] {
        press(&mut app, k);
        assert_eq!(app.mode_label(), "NORMAL", "{k} cannot enter Insert mode");
    }
    assert_eq!(text_of(&app), "select 1;", "nothing changed");
}

#[test]
fn every_editing_command_is_refused_while_disconnected_and_leaves_the_state_alone() {
    let mut app = sql_app("select 1;\nselect 2;", Config::default());
    at(&mut app, 0, 3);
    let before = (app.buffer.cursor, app.buffer.goal_col);
    for k in ["x", "X", "D", "C", "d d", "d w", "c c", "p", "P", "u", "ctrl+r", "o", "O", "r"] {
        press(&mut app, k);
        assert_eq!(text_of(&app), "select 1;\nselect 2;", "after {k}");
        assert_eq!(app.mode_label(), "NORMAL", "after {k}");
        press(&mut app, "esc");
    }
    assert_eq!((app.buffer.cursor, app.buffer.goal_col), before, "refused edits do not move the cursor");
    assert!(!app.buffer.is_modified());
}

#[test]
fn navigation_selection_and_copy_still_work_while_disconnected() {
    let (mut app, clip) = {
        let (mut a, c) = app_with("select 1;\nselect 2;");
        a.set_lang(Some(&syntax::SQL));
        (a, c)
    };
    press(&mut app, "j l l v l l l ctrl+c");
    assert_eq!(*clip.0.lock().unwrap(), "lect", "copying is not an edit");
    press(&mut app, "esc g g");
    assert_eq!(app.buffer.cursor_line_col(), (0, 0));
    press(&mut app, "ctrl+f");
    typed(&mut app, "select");
    assert!(screen(&mut app).join("\n").contains("1 of 2"), "find works");
}

#[test]
fn modeless_typing_is_refused_with_the_message_and_does_not_move_the_cursor() {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    let mut app = sql_app("select", cfg);
    at(&mut app, 0, 6);
    typed(&mut app, "x");
    press(&mut app, "enter tab backspace delete");
    assert_eq!(text_of(&app), "select");
    assert_eq!(app.buffer.cursor, 6);
    assert_eq!(screen(&mut app)[23].trim_end(), DISCONNECTED_MSG);
    app.handle_paste("pasted");
    assert_eq!(text_of(&app), "select", "paste is refused too");
}

#[test]
fn after_connecting_editing_works_and_the_message_goes_away() {
    let mut app = sql_app("select 1;", Config::default());
    connect_fake(&mut app, Engine::Postgres);
    assert!(!screen(&mut app)[23].contains("Not connected"));
    press(&mut app, "A");
    assert_eq!(app.mode_label(), "INSERT");
    typed(&mut app, " -- ok");
    assert_eq!(text_of(&app), "select 1; -- ok");
}

#[test]
fn disconnecting_blocks_editing_again() {
    let mut app = sql_app("select 1;", Config::default());
    connect_fake(&mut app, Engine::Postgres);
    cmd(&mut app, "disconnect");
    assert!(app.sql.session.is_none());
    press(&mut app, "i");
    assert_eq!(app.mode_label(), "NORMAL");
}

#[test]
fn other_languages_are_not_affected() {
    let (mut app, _) = app_with("package main");
    app.set_lang(Some(&syntax::GO));
    press(&mut app, "A");
    typed(&mut app, "x");
    assert_eq!(text_of(&app), "package mainx");
    assert!(!screen(&mut app)[23].contains("Not connected"));
}

#[test]
fn the_block_can_be_switched_off_in_the_config() {
    let mut cfg = Config::default();
    cfg.sql.block_editing_when_disconnected = false;
    let mut app = sql_app("select 1;", cfg);
    press(&mut app, "A");
    typed(&mut app, "!");
    assert_eq!(text_of(&app), "select 1;!");
    assert!(!screen(&mut app)[23].contains("Not connected"));
}

#[test]
fn a_snippet_can_still_be_inserted_while_disconnected() {
    let mut app = sql_app("", Config::default());
    press(&mut app, "alt+s");
    typed(&mut app, "select limit");
    press(&mut app, "enter");
    assert!(text_of(&app).contains("LIMIT 10 OFFSET 0"), "{:?}", text_of(&app));
    assert_eq!(app.focus, Focus::Editor);
}

#[test]
fn running_without_a_connection_explains_how_to_connect() {
    let mut app = sql_app("select 1;", Config::default());
    press(&mut app, "ctrl+enter");
    let m = screen(&mut app)[23].clone();
    assert!(m.contains("Not connected") && m.contains(":connect"), "{m:?}");
}

// ---------------------------------------------------------------- Ctrl+Enter

#[test]
fn ctrl_enter_runs_the_selection_when_there_is_one() {
    let mut app = sql_app("select 1;\nselect 2;\nselect 3;", Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    at(&mut app, 0, 0);
    press(&mut app, "v j l l l l l l l l");
    press(&mut app, "ctrl+enter");
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["select 1;\nselect 2;"], "exactly the selection, which may span statements");
}

#[test]
fn ctrl_enter_without_a_selection_runs_the_statement_under_the_cursor() {
    let mut app = sql_app("select 1;\nselect\n  2,\n  3;\nselect 4;", Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    at(&mut app, 2, 3); // inside the multi-line second statement
    press(&mut app, "ctrl+enter");
    wait_idle(&mut app);
    at(&mut app, 0, 2);
    press(&mut app, "ctrl+enter");
    wait_idle(&mut app);
    at(&mut app, 4, 4);
    press(&mut app, "ctrl+enter");
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["select\n  2,\n  3;", "select 1;", "select 4;"]);
}

#[test]
fn between_statements_the_previous_one_runs() {
    let mut app = sql_app("select 1;\n\n\nselect 2;", Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    at(&mut app, 1, 0);
    press(&mut app, "ctrl+enter");
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["select 1;"]);
}

#[test]
fn semicolons_inside_strings_do_not_split_the_statement() {
    let mut app = sql_app("select 'a;b' as x, 2;\nselect 3;", Config::default());
    let log = connect_fake(&mut app, Engine::MariaDb);
    at(&mut app, 0, 5);
    press(&mut app, "ctrl+enter");
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["select 'a;b' as x, 2;"]);
}

#[test]
fn postgres_dollar_quoted_bodies_stay_in_one_statement_but_not_on_mariadb() {
    let text = "do $$ begin perform 1; perform 2; end $$;\nselect 9;";
    let mut app = sql_app(text, Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    at(&mut app, 0, 15);
    press(&mut app, "ctrl+enter");
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["do $$ begin perform 1; perform 2; end $$;"]);
}

#[test]
fn empty_buffer_or_only_comments_has_nothing_to_run() {
    let mut app = sql_app("-- just a comment\n", Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    press(&mut app, "ctrl+enter");
    assert!(screen(&mut app)[23].contains("nothing to run"));
    assert!(ran(&log).is_empty());
}

#[test]
fn ctrl_enter_works_in_every_mode_and_with_the_find_bar_open() {
    let mut app = sql_app("select 1;", Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    press(&mut app, "A");
    press(&mut app, "ctrl+enter"); // Insert mode
    wait_idle(&mut app);
    press(&mut app, "esc ctrl+f");
    press(&mut app, "ctrl+enter"); // find bar focused
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["select 1;", "select 1;"]);
}

#[test]
fn f5_no_longer_runs_anything() {
    let mut app = sql_app("select 1;", Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    press(&mut app, "v $ f5");
    assert!(ran(&log).is_empty());
    assert!(app.sql.result.is_none());
}

// ---------------------------------------------------------------- fallback key and run-line

#[test]
fn the_fallback_key_runs_the_same_thing_as_ctrl_enter() {
    let mut app = sql_app("select 1;\nselect 2;", Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    at(&mut app, 1, 2);
    press(&mut app, "alt+enter");
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["select 2;"]);
}

#[test]
fn the_fallback_key_is_configurable() {
    let mut cfg = Config::default();
    cfg.sql.run_fallback_key = "f9".into();
    let mut app = sql_app("select 1;", cfg);
    let log = connect_fake(&mut app, Engine::Postgres);
    press(&mut app, "alt+enter");
    assert!(ran(&log).is_empty(), "the old default no longer applies");
    press(&mut app, "f9");
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["select 1;"]);
}

#[test]
fn invalid_key_settings_warn_and_fall_back() {
    let (c, w) = Config::from_toml("[sql]\nrun_fallback_key = \"hyper+nonsense\"\nrun_line_key = \"\"\n");
    assert_eq!((c.sql.run_fallback_key.as_str(), c.sql.run_line_key.as_str()), ("alt+enter", "alt+l"));
    assert_eq!(w.len(), 2, "{w:?}");
}

#[test]
fn run_line_runs_only_the_current_line() {
    let mut app = sql_app("select\n  1,\n  2;\nselect 3;", Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    at(&mut app, 1, 1);
    press(&mut app, "alt+l");
    wait_idle(&mut app);
    cmd(&mut app, "runline");
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["  1,", "  1,"], "just that line, not the whole statement");
}

#[test]
fn run_line_on_an_empty_line_says_so() {
    let mut app = sql_app("select 1;\n\n", Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    at(&mut app, 1, 0);
    cmd(&mut app, "runline");
    assert!(screen(&mut app)[23].contains("empty"));
    assert!(ran(&log).is_empty());
}

#[test]
fn run_line_key_is_configurable() {
    let mut cfg = Config::default();
    cfg.sql.run_line_key = "f8".into();
    let mut app = sql_app("select 1;", cfg);
    let log = connect_fake(&mut app, Engine::Postgres);
    press(&mut app, "alt+l");
    assert!(ran(&log).is_empty());
    press(&mut app, "f8");
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["select 1;"]);
}

#[test]
fn colon_commands_run_connect_and_buffer() {
    let mut app = sql_app("select 1;\nselect 2;", Config::default());
    let log = connect_fake(&mut app, Engine::Postgres);
    at(&mut app, 1, 0);
    cmd(&mut app, "run");
    wait_idle(&mut app);
    cmd(&mut app, "runbuffer");
    wait_idle(&mut app);
    assert_eq!(ran(&log), vec!["select 2;", "select 1;\nselect 2;"]);
    cmd(&mut app, "connect nope");
    let m = screen(&mut app)[23].clone();
    assert!(m.contains("no connection profiles") || m.contains("no profile"), "{m:?}");
}

#[test]
fn error_positions_are_relative_to_the_buffer_for_statement_runs() {
    struct Failing;
    impl Driver for Failing {
        fn execute(&mut self, _sql: &str, _l: usize) -> Result<ResultSet, DbError> {
            Err(DbError { category: kuro::sql::ErrorCategory::Syntax, message: "bad".into(), line: Some(2), column: Some(3), code: None })
        }
        fn canceller(&self) -> Canceller {
            Arc::new(|| Ok(()))
        }
        fn label(&self) -> String {
            "x".into()
        }
        fn engine(&self) -> Engine {
            Engine::Postgres
        }
    }
    let mut app = sql_app("select 1;\n\nselect\n  oops;", Config::default());
    app.sql.session = Some(Session::new(Box::new(Failing)));
    at(&mut app, 3, 2);
    press(&mut app, "ctrl+enter");
    wait_idle(&mut app);
    let e = app.sql.result.as_ref().unwrap().outcome.as_ref().unwrap_err();
    assert_eq!(e.line, Some(2 + 2), "the statement starts on buffer line 3 (index 2), so its line 2 is buffer line 4");
}

// ---------------------------------------------------------------- kitty protocol notice

#[test]
fn a_terminal_without_the_kitty_protocol_gets_a_clear_message_naming_the_fallback_key() {
    let mut app = sql_app("select 1;", Config::default());
    app.set_kitty(false);
    let m = screen(&mut app)[23].clone();
    assert!(m.contains("Ctrl+Enter") && m.contains("Alt+Enter") && m.contains(":run"), "{m:?}");
    assert!(m.contains("kitty"), "says why");
    assert!(m.chars().count() <= 80, "fits an 80-column terminal without being cut: {m:?}");
}

#[test]
fn the_notice_names_a_configured_fallback_key() {
    let mut cfg = Config::default();
    cfg.sql.run_fallback_key = "f9".into();
    let mut app = sql_app("select 1;", cfg);
    app.set_kitty(false);
    assert!(screen(&mut app)[23].contains("F9"));
}

#[test]
fn no_notice_with_kitty_or_in_non_sql_buffers_and_only_once() {
    let mut app = sql_app("select 1;", Config::default());
    app.set_kitty(true);
    assert!(!screen(&mut app)[23].contains("Ctrl+Enter"));
    let (mut app, _) = app_with("plain");
    app.set_kitty(false);
    assert!(!screen(&mut app)[23].contains("Ctrl+Enter"), "not an SQL buffer");
    app.set_lang(Some(&syntax::SQL));
    assert!(app.msg.as_deref().is_some_and(|m| m.contains("Ctrl+Enter")), "shown when the buffer becomes SQL");
    app.msg = None;
    app.set_lang(Some(&syntax::GO));
    app.set_lang(Some(&syntax::SQL));
    assert!(app.msg.is_none(), "only once");
}

#[test]
fn plain_enter_is_just_a_newline_so_legacy_terminals_cannot_trigger_a_run_by_accident() {
    let mut cfg = Config::default();
    cfg.sql.block_editing_when_disconnected = false;
    let mut app = sql_app("select 1;", cfg);
    let log = connect_fake(&mut app, Engine::Postgres);
    press(&mut app, "A enter");
    assert!(ran(&log).is_empty());
    assert_eq!(app.buffer.line_count(), 2);
}
