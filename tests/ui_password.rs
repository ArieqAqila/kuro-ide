//! Password handling (Part 4.6): memory only, reused for the same profile, never for another, dropped when wrong,
//! gone on exit. A fake connector stands in for the database and checks the password it is given.

mod common;
use common::fakedb::Recorder;
use common::ui::*;
use kuro::app::{App, Focus};
use kuro::config::Config;
use kuro::sql::profile::Engine;
use kuro::sql::{DbError, ErrorCategory};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RIGHT: &str = "correct-horse-battery-UNIQUE-7f3a";

type Calls = Arc<Mutex<Vec<(String, Option<String>)>>>;

fn profiles_file(dir: &std::path::Path) -> std::path::PathBuf {
    let f = dir.join("connections.toml");
    std::fs::write(
        &f,
        r#"
[profiles.a]
driver = "postgres"
host = "db-a.example"
user = "u"
password = "prompt"
database = "x"

[profiles.b]
driver = "postgres"
host = "db-b.example"
user = "u"
password = "prompt"
database = "x"

[profiles.noprompt]
driver = "mariadb"
host = "db-c.example"
user = "u"
password = "env:KURO_TEST_PW_FOR_NOPROMPT"
"#,
    )
    .unwrap();
    f
}

/// An app whose connector accepts only `RIGHT` and records every attempt.
fn app(dir: &std::path::Path) -> (App, Calls) {
    let mut cfg = Config::default();
    cfg.sql.connections_file = profiles_file(dir).display().to_string();
    let (mut app, _) = app_cfg("select 1;", cfg);
    let calls: Calls = Arc::default();
    let log = calls.clone();
    app.set_connector(Arc::new(move |r| {
        log.lock().unwrap().push((r.host.clone(), r.password.clone()));
        if r.password.as_deref() == Some(RIGHT) || r.password.is_none() {
            Ok(Box::new(Recorder { engine: Engine::Postgres, log: Arc::default() }) as Box<dyn kuro::sql::Driver>)
        } else {
            Err(DbError {
                category: ErrorCategory::Execution,
                message: "password authentication failed".into(),
                line: None,
                column: None,
                code: Some("28P01".into()),
            })
        }
    }));
    (app, calls)
}

fn settle(app: &mut App) {
    let t = Instant::now();
    while app.sql.connecting.is_some() {
        assert!(t.elapsed() < Duration::from_secs(10), "connect never finished");
        std::thread::yield_now();
        app.tick();
    }
}

fn connect(app: &mut App, name: &str) {
    press(app, ":");
    typed(app, &format!("connect {name}"));
    press(app, "enter");
}

fn type_password(app: &mut App, pw: &str) {
    assert_eq!(app.focus, Focus::Prompt, "a password prompt should be showing");
    typed(app, pw);
    press(app, "enter");
    settle(app);
}

#[test]
fn the_first_connection_asks_for_the_password_hidden() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, calls) = app(d.path());
    connect(&mut a, "a");
    assert_eq!(a.focus, Focus::Prompt);
    typed(&mut a, RIGHT);
    let line = screen(&mut a)[23].clone();
    assert!(line.contains("password for a"), "{line:?}");
    assert!(!line.contains(RIGHT) && !line.contains("correct"), "never echoed: {line:?}");
    assert!(line.contains("•"), "masked");
    press(&mut a, "enter");
    settle(&mut a);
    assert!(a.sql.session.is_some());
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(calls.lock().unwrap()[0], ("db-a.example".to_string(), Some(RIGHT.to_string())));
}

#[test]
fn reconnecting_to_the_same_profile_reuses_the_password_without_asking() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, calls) = app(d.path());
    connect(&mut a, "a");
    type_password(&mut a, RIGHT);
    press(&mut a, ":");
    typed(&mut a, "disconnect");
    press(&mut a, "enter");
    assert!(a.sql.session.is_none());
    connect(&mut a, "a");
    assert_eq!(a.focus, Focus::Editor, "no prompt the second time");
    settle(&mut a);
    assert!(a.sql.session.is_some(), "connected again");
    let c = calls.lock().unwrap();
    assert_eq!(c.len(), 2);
    assert_eq!(c[1].1.as_deref(), Some(RIGHT), "with the remembered password");
}

#[test]
fn a_different_profile_asks_again() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, _) = app(d.path());
    connect(&mut a, "a");
    type_password(&mut a, RIGHT);
    connect(&mut a, "b");
    assert_eq!(a.focus, Focus::Prompt, "profile b has its own password");
    assert!(screen(&mut a)[23].contains("password for b"));
    press(&mut a, "esc");
    connect(&mut a, "a");
    assert_eq!(a.focus, Focus::Editor, "and a is still remembered");
    settle(&mut a);
}

#[test]
fn a_wrong_password_is_not_kept_and_the_next_connect_asks_again() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, calls) = app(d.path());
    connect(&mut a, "a");
    type_password(&mut a, "definitely-wrong");
    assert!(a.sql.session.is_none());
    assert!(a.passwords.is_empty(), "a rejected password is never stored");
    let s = screen(&mut a).join("\n");
    assert!(s.contains("wrong password") || s.contains("authentication failed"), "{s}");
    connect(&mut a, "a");
    assert_eq!(a.focus, Focus::Prompt, "asks again");
    type_password(&mut a, RIGHT);
    assert!(a.sql.session.is_some());
    assert_eq!(calls.lock().unwrap().len(), 2);
}

#[test]
fn a_remembered_password_that_stops_working_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, _) = app(d.path());
    connect(&mut a, "a");
    type_password(&mut a, RIGHT);
    press(&mut a, ":");
    typed(&mut a, "disconnect");
    press(&mut a, "enter");
    // the account's password is changed on the server: the connector now rejects the old one
    let calls: Calls = Arc::default();
    a.set_connector(Arc::new(move |_| {
        calls.lock().unwrap().push((String::new(), None));
        Err(DbError {
            category: ErrorCategory::Execution,
            message: "Access denied for user".into(),
            line: None,
            column: None,
            code: Some("1045".into()),
        })
    }));
    connect(&mut a, "a");
    settle(&mut a);
    assert!(a.passwords.is_empty(), "the stale password was removed after the server refused it");
    connect(&mut a, "a");
    assert_eq!(a.focus, Focus::Prompt, "so the user is asked for the new one");
}

#[test]
fn other_failures_do_not_store_the_password_either() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, _) = app(d.path());
    a.set_connector(Arc::new(|_| Err(DbError::execution("connection refused"))));
    connect(&mut a, "a");
    type_password(&mut a, RIGHT);
    assert!(a.passwords.is_empty(), "nothing is remembered unless a connection actually succeeded");
}

#[test]
fn profiles_that_do_not_prompt_are_never_cached() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, calls) = app(d.path());
    // env var not set: resolve fails with a clear message, nothing cached, nothing prompted
    connect(&mut a, "noprompt");
    assert_eq!(a.focus, Focus::Editor);
    assert!(a.passwords.is_empty());
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn forget_clears_one_profile_or_all() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, _) = app(d.path());
    connect(&mut a, "a");
    type_password(&mut a, RIGHT);
    connect(&mut a, "b");
    type_password(&mut a, RIGHT);
    assert_eq!(a.passwords.len(), 2);
    press(&mut a, ":");
    typed(&mut a, "sql forget a");
    press(&mut a, "enter");
    assert_eq!(a.passwords.len(), 1);
    assert!(screen(&mut a)[23].contains("forgot 1 stored password"));
    press(&mut a, ":");
    typed(&mut a, "sql forget");
    press(&mut a, "enter");
    assert!(a.passwords.is_empty());
    connect(&mut a, "a");
    assert_eq!(a.focus, Focus::Prompt);
}

#[test]
fn the_cache_is_gone_when_the_app_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, _) = app(d.path());
    connect(&mut a, "a");
    type_password(&mut a, RIGHT);
    assert_eq!(a.passwords.len(), 1);
    let token = a.passwords.alive_token();
    assert!(token.upgrade().is_some());
    drop(a);
    assert!(token.upgrade().is_none(), "the password cache does not outlive the editor");
}

#[test]
fn the_password_never_shows_up_in_debug_output_or_on_screen() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, _) = app(d.path());
    connect(&mut a, "a");
    typed(&mut a, RIGHT);
    for row in screen(&mut a) {
        assert!(!row.contains("correct-horse"), "{row:?}");
    }
    press(&mut a, "enter");
    settle(&mut a);
    for row in screen(&mut a) {
        assert!(!row.contains("correct-horse"), "{row:?}");
    }
    assert!(!format!("{:?}", a.passwords).contains("correct-horse"));
    let key = a.passwords.alive_token();
    assert!(key.upgrade().is_some());
}

#[test]
fn nothing_is_written_to_the_filesystem_when_a_password_is_cached() {
    // everything the app could write goes under these directories: none may contain the password afterwards
    let d = tempfile::tempdir().unwrap();
    let (mut a, _) = app(d.path());
    connect(&mut a, "a");
    type_password(&mut a, RIGHT);
    connect(&mut a, "a");
    settle(&mut a);
    fn scan(dir: &std::path::Path, hits: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                scan(&p, hits);
            } else if std::fs::read(&p).is_ok_and(|b| b.windows(RIGHT.len()).any(|w| w == RIGHT.as_bytes())) {
                hits.push(p.display().to_string());
            }
        }
    }
    let mut hits = vec![];
    scan(d.path(), &mut hits);
    assert!(hits.is_empty(), "password found on disk: {hits:?}");
}
