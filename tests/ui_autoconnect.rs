//! `sql.auto_connect`: the first SQL buffer shown connects to the configured profile by itself, once per session.
//! A fake connector stands in for the database and records every attempt.

mod common;
use common::fakedb::Recorder;
use common::ui::*;
use kuro::app::{App, Focus, PromptKind};
use kuro::config::Config;
use kuro::sql::profile::Engine;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Calls = Arc<Mutex<Vec<String>>>;

fn profiles_file(dir: &std::path::Path) -> std::path::PathBuf {
    let f = dir.join("connections.toml");
    std::fs::write(
        &f,
        r#"
[profiles.local]
driver = "mariadb"
host = "db-local.example"
user = "u"
database = "x"

[profiles.asks]
driver = "mariadb"
host = "db-asks.example"
user = "u"
password = "prompt"
database = "x"
"#,
    )
    .unwrap();
    f
}

/// An app (showing a plain text buffer) whose connector accepts everything and records the host it was given.
fn app(dir: &std::path::Path, auto_connect: &str) -> (App, Calls) {
    let mut cfg = Config::default();
    cfg.sql.connections_file = profiles_file(dir).display().to_string();
    cfg.sql.auto_connect = auto_connect.to_string();
    let (mut app, _) = app_cfg("", cfg);
    let calls: Calls = Arc::default();
    let log = calls.clone();
    app.set_connector(Arc::new(move |r| {
        log.lock().unwrap().push(r.host.clone());
        Ok(Box::new(Recorder { engine: Engine::MariaDb, log: Arc::default() }) as Box<dyn kuro::sql::Driver>)
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

fn file(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, "select 1;\n").unwrap();
    p
}

#[test]
fn opening_a_sql_file_connects_to_the_profile() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, calls) = app(d.path(), "local");
    a.open_file(&file(d.path(), "q.sql"));
    settle(&mut a);
    assert!(a.sql.session.is_some(), "connected");
    assert_eq!(*calls.lock().unwrap(), vec!["db-local.example".to_string()]);
}

#[test]
fn a_prompt_profile_asks_for_the_password_right_away() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, calls) = app(d.path(), "asks");
    a.open_file(&file(d.path(), "q.sql"));
    assert_eq!(a.focus, Focus::Prompt);
    assert!(matches!(a.prompt.as_ref().map(|p| &p.kind), Some(PromptKind::Password(n)) if n == "asks"));
    assert!(calls.lock().unwrap().is_empty(), "nothing is sent before the password is typed");
}

#[test]
fn other_files_do_not_connect() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, calls) = app(d.path(), "local");
    a.open_file(&file(d.path(), "notes.txt"));
    settle(&mut a);
    assert!(a.sql.session.is_none());
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn empty_setting_never_connects() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, calls) = app(d.path(), "");
    a.open_file(&file(d.path(), "q.sql"));
    settle(&mut a);
    assert!(a.sql.session.is_none());
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn it_is_tried_only_once_per_session() {
    let d = tempfile::tempdir().unwrap();
    let (mut a, calls) = app(d.path(), "local");
    a.open_file(&file(d.path(), "a.sql"));
    settle(&mut a);
    press(&mut a, ":");
    typed(&mut a, "disconnect");
    press(&mut a, "enter");
    assert!(a.sql.session.is_none(), "disconnected");
    a.open_file(&file(d.path(), "b.sql"));
    settle(&mut a);
    assert!(a.sql.session.is_none(), "an explicit :disconnect is respected");
    assert_eq!(calls.lock().unwrap().len(), 1);
}
