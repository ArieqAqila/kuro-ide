//! SQL client: drivers run on a worker thread so the UI never blocks and a
//! running query can be cancelled from the UI thread.

pub mod classify;
mod my;
mod pg;
pub mod profile;

use crate::table::Column;
pub use classify::ErrorCategory;
use profile::{Engine, Resolved};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResultSet {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<String>>,
    /// More rows existed than `row_limit`; `total_rows` has the real count.
    pub truncated: bool,
    pub total_rows: usize,
    /// For statements that return no rows (INSERT, DDL...).
    pub affected: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbError {
    pub category: ErrorCategory,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// SQLSTATE (PostgreSQL) or vendor code (MariaDB), when available.
    pub code: Option<String>,
}

impl DbError {
    pub fn execution(message: impl Into<String>) -> Self {
        DbError { category: ErrorCategory::Execution, message: message.into(), line: None, column: None, code: None }
    }

    /// Positions are relative to the executed text; shift them to buffer coordinates
    /// when only a selection starting at (`line0`, `col0`), both 0-based, was run.
    pub fn shift(&mut self, line0: usize, col0: usize) {
        if let Some(l) = self.line {
            if l == 1 {
                self.column = self.column.map(|c| c + col0);
            }
            self.line = Some(l + line0);
        }
    }
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.category.label())?;
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, " at line {l}, col {c}")?,
            (Some(l), None) => write!(f, " at line {l}")?,
            _ => {}
        }
        write!(f, ": {}", self.message)
    }
}

pub type Canceller = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

pub trait Driver: Send {
    fn execute(&mut self, sql: &str, row_limit: usize) -> Result<ResultSet, DbError>;
    fn canceller(&self) -> Canceller;
    fn label(&self) -> String;
}

pub fn connect(r: &Resolved) -> Result<Box<dyn Driver>, DbError> {
    match r.engine {
        Engine::Postgres => Ok(Box::new(pg::PgDriver::connect(r)?)),
        Engine::MariaDb => Ok(Box::new(my::MyDriver::connect(r)?)),
    }
}

#[derive(Debug)]
pub struct QueryReport {
    pub elapsed: Duration,
    pub result: Result<ResultSet, DbError>,
}

enum Cmd {
    Run(String, usize),
    Quit,
}

pub struct Session {
    tx: Sender<Cmd>,
    rx: Receiver<QueryReport>,
    canceller: Canceller,
    pub label: String,
    pub running_since: Option<Instant>,
}

impl Session {
    pub fn new(mut driver: Box<dyn Driver>) -> Session {
        let (tx, cmd_rx) = channel::<Cmd>();
        let (rep_tx, rx) = channel::<QueryReport>();
        let canceller = driver.canceller();
        let label = driver.label();
        std::thread::spawn(move || {
            while let Ok(Cmd::Run(sql, limit)) = cmd_rx.recv() {
                let t = Instant::now();
                let result = driver.execute(&sql, limit);
                // elapsed is measured around the database call only
                if rep_tx.send(QueryReport { elapsed: t.elapsed(), result }).is_err() {
                    break;
                }
            }
        });
        Session { tx, rx, canceller, label, running_since: None }
    }

    pub fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    pub fn run(&mut self, sql: &str, row_limit: usize) -> Result<(), String> {
        if self.is_running() {
            return Err("a query is already running (cancel it first)".into());
        }
        self.tx.send(Cmd::Run(sql.to_string(), row_limit)).map_err(|_| "connection worker has stopped".to_string())?;
        self.running_since = Some(Instant::now());
        Ok(())
    }

    pub fn cancel(&self) -> Result<(), String> {
        if !self.is_running() {
            return Err("no query is running".into());
        }
        (self.canceller)()
    }

    /// Non-blocking: the finished report, if any.
    pub fn poll(&mut self) -> Option<QueryReport> {
        match self.rx.try_recv() {
            Ok(r) => {
                self.running_since = None;
                Some(r)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.running_since = None;
                Some(QueryReport { elapsed: Duration::ZERO, result: Err(DbError::execution("connection lost: worker stopped")) })
            }
        }
    }

    /// Blocking variant for tests.
    pub fn wait(&mut self, timeout: Duration) -> Option<QueryReport> {
        let r = self.rx.recv_timeout(timeout).ok();
        if r.is_some() {
            self.running_since = None;
        }
        r
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
    }
}

pub fn format_duration(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 1.0 {
        format!("{:.2} ms", ms)
    } else if ms < 1000.0 {
        format!("{:.1} ms", ms)
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// The status line shown under the result table.
pub fn status_text(r: &QueryReport) -> String {
    let t = format_duration(r.elapsed);
    match &r.result {
        Ok(rs) if rs.columns.is_empty() => match rs.affected {
            Some(n) => format!("OK · {n} row{} affected · {t}", if n == 1 { "" } else { "s" }),
            None => format!("OK · {t}"),
        },
        Ok(rs) => {
            let n = rs.total_rows;
            let mut s = format!("{n} row{} · {t}", if n == 1 { "" } else { "s" });
            if rs.truncated {
                s.push_str(&format!(" · showing first {} (row_limit)", rs.rows.len()));
            }
            s
        }
        Err(e) => {
            let mut s = format!("✖ {} · {t}", e.category.label());
            match (e.line, e.column) {
                (Some(l), Some(c)) => s.push_str(&format!(" · line {l}, col {c}")),
                (Some(l), None) => s.push_str(&format!(" · line {l}")),
                _ => {}
            }
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(format_duration(Duration::from_micros(250)), "0.25 ms");
        assert_eq!(format_duration(Duration::from_millis(12)), "12.0 ms");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.50 s");
    }

    fn ok(rows: usize, truncated: bool) -> QueryReport {
        QueryReport {
            elapsed: Duration::from_millis(5),
            result: Ok(ResultSet {
                columns: vec![Column { name: "a".into(), type_name: "int".into() }],
                rows: vec![vec!["1".into()]; rows.min(3)],
                truncated,
                total_rows: rows,
                affected: None,
            }),
        }
    }

    #[test]
    fn status_for_rows() {
        assert_eq!(status_text(&ok(1, false)), "1 row · 5.0 ms");
        assert_eq!(status_text(&ok(12, false)), "12 rows · 5.0 ms");
        assert!(status_text(&ok(500, true)).contains("showing first 3"));
    }

    #[test]
    fn status_for_affected_and_errors() {
        let r = QueryReport { elapsed: Duration::from_millis(2), result: Ok(ResultSet { affected: Some(3), ..Default::default() }) };
        assert_eq!(status_text(&r), "OK · 3 rows affected · 2.0 ms");
        let e = QueryReport {
            elapsed: Duration::from_millis(2),
            result: Err(DbError { category: ErrorCategory::Syntax, message: "boom".into(), line: Some(2), column: Some(5), code: None }),
        };
        assert_eq!(status_text(&e), "✖ syntax error · 2.0 ms · line 2, col 5");
    }

    #[test]
    fn error_shift_maps_selection_to_buffer_coordinates() {
        let mut e = DbError { category: ErrorCategory::Syntax, message: String::new(), line: Some(1), column: Some(3), code: None };
        e.shift(10, 4);
        assert_eq!((e.line, e.column), (Some(11), Some(7)));
        let mut e = DbError { category: ErrorCategory::Syntax, message: String::new(), line: Some(2), column: Some(3), code: None };
        e.shift(10, 4);
        assert_eq!((e.line, e.column), (Some(12), Some(3)), "column only shifts on the first line");
    }

    #[test]
    fn display_includes_category_position_and_message() {
        let e = DbError { category: ErrorCategory::Runtime, message: "division by zero".into(), line: Some(1), column: Some(8), code: None };
        assert_eq!(e.to_string(), "runtime error at line 1, col 8: division by zero");
    }

    struct Fake;
    impl Driver for Fake {
        fn execute(&mut self, sql: &str, _l: usize) -> Result<ResultSet, DbError> {
            if sql == "slow" {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(ResultSet { affected: Some(1), ..Default::default() })
        }
        fn canceller(&self) -> Canceller {
            Arc::new(|| Ok(()))
        }
        fn label(&self) -> String {
            "fake".into()
        }
    }

    #[test]
    fn session_runs_one_query_at_a_time_and_times_it() {
        let mut s = Session::new(Box::new(Fake));
        s.run("slow", 10).unwrap();
        assert!(s.is_running());
        assert!(s.run("x", 10).is_err(), "second query while running is refused");
        let rep = s.wait(Duration::from_secs(5)).unwrap();
        assert!(rep.elapsed >= Duration::from_millis(50));
        assert!(!s.is_running());
        s.run("fast", 10).unwrap();
        assert!(s.wait(Duration::from_secs(5)).is_some());
        assert!(s.cancel().is_err(), "nothing to cancel");
    }
}
