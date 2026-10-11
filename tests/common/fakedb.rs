//! Fake database drivers for tests that need a connection but not a database.
#![allow(dead_code)]

use kuro::app::App;
use kuro::sql::profile::Engine;
use kuro::sql::{Canceller, DbError, Driver, ErrorCategory, ResultSet, Session};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type Log = Arc<Mutex<Vec<String>>>;

/// Records every statement it is asked to run; returns an empty OK result.
pub struct Recorder {
    pub engine: Engine,
    pub log: Log,
}

impl Driver for Recorder {
    fn execute(&mut self, sql: &str, _l: usize) -> Result<ResultSet, DbError> {
        self.log.lock().unwrap().push(sql.to_string());
        Ok(ResultSet { affected: Some(1), ..Default::default() })
    }
    fn canceller(&self) -> Canceller {
        Arc::new(|| Ok(()))
    }
    fn label(&self) -> String {
        "recorder".into()
    }
    fn engine(&self) -> Engine {
        self.engine
    }
}

/// Blocks until cancelled, then fails like a cancelled statement.
pub struct Slow {
    pub cancelled: Arc<AtomicBool>,
}

impl Driver for Slow {
    fn execute(&mut self, _sql: &str, _l: usize) -> Result<ResultSet, DbError> {
        let t = Instant::now();
        while !self.cancelled.swap(false, Ordering::SeqCst) && t.elapsed() < Duration::from_secs(20) {
            std::thread::yield_now();
        }
        Err(DbError {
            category: ErrorCategory::Execution,
            message: "canceling statement due to user request".into(),
            line: None,
            column: None,
            code: Some("57014".into()),
        })
    }
    fn canceller(&self) -> Canceller {
        let c = self.cancelled.clone();
        Arc::new(move || {
            c.store(true, Ordering::SeqCst);
            Ok(())
        })
    }
    fn label(&self) -> String {
        "slow".into()
    }
    fn engine(&self) -> Engine {
        Engine::Postgres
    }
}

pub fn connect_recorder(app: &mut App, engine: Engine) -> Log {
    let log: Log = Arc::default();
    app.sql.session = Some(Session::new(Box::new(Recorder { engine, log: log.clone() })));
    log
}

pub fn connect_slow(app: &mut App) {
    app.sql.session = Some(Session::new(Box::new(Slow { cancelled: Arc::default() })));
}

/// Tick until the running query has produced its result (bounded).
pub fn wait_idle(app: &mut App) {
    let t = Instant::now();
    while app.sql.session.as_ref().is_some_and(|s| s.is_running()) {
        assert!(t.elapsed() < Duration::from_secs(10), "query never finished");
        std::thread::yield_now();
        app.tick();
    }
}

pub fn ran(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}
