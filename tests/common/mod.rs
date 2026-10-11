//! Test helpers: throwaway Docker databases bound to 127.0.0.1 only.
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod colorscan;
pub mod fakedb;
pub mod pty;
pub mod snap;
pub mod sqlview;
pub mod ui;

use kuro::sql::{self, DbError, QueryReport, Session, profile::*};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// `Some(reason)` when Docker cannot be used. With `KURO_REQUIRE_DOCKER=1` (CI) this panics instead.
pub fn docker_unavailable() -> Option<String> {
    let ok = Command::new("docker").arg("info").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    if ok {
        return None;
    }
    let msg = "docker is not available (daemon not running or binary missing)".to_string();
    if std::env::var("KURO_REQUIRE_DOCKER").is_ok_and(|v| v == "1") {
        panic!("{msg}, but KURO_REQUIRE_DOCKER=1");
    }
    eprintln!("SKIPPED integration test: {msg}. Run them with docker to cover MariaDB/PostgreSQL.");
    Some(msg)
}

pub struct Container {
    pub name: String,
    pub port: u16,
    pub password: String,
}

fn docker(args: &[&str]) -> (bool, String) {
    let out = Command::new("docker").args(args).output().expect("docker");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).trim().to_string() + &String::from_utf8_lossy(&out.stderr))
}

impl Container {
    pub fn start(tag: &str, image: &str, inner_port: u16, env: &[(&str, &str)]) -> Container {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("kuro-test-{tag}-{}-{nanos}", std::process::id());
        let password = format!("t{}x{nanos}", std::process::id());
        let mut args: Vec<String> = vec!["run".into(), "-d".into(), "--rm".into(), "--name".into(), name.clone()];
        // bind to loopback only, random host port
        args.extend(["-p".into(), format!("127.0.0.1::{inner_port}")]);
        for (k, v) in env {
            args.extend(["-e".into(), format!("{k}={}", v.replace("{pw}", &password))]);
        }
        args.push(image.into());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (ok, out) = docker(&refs);
        assert!(ok, "docker run failed: {out}");
        let (ok, out) = docker(&["port", &name, &format!("{inner_port}/tcp")]);
        assert!(ok, "docker port failed: {out}");
        let port: u16 = out.lines().next().unwrap().rsplit(':').next().unwrap().trim().parse().expect("port");
        Container { name, port, password }
    }

    pub fn wait_log(&self, needle: &str, times: usize) {
        let t = Instant::now();
        loop {
            let (_, logs) = docker(&["logs", &self.name]);
            if logs.matches(needle).count() >= times {
                return;
            }
            assert!(t.elapsed() < Duration::from_secs(120), "container never became ready; logs:\n{logs}");
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    pub fn kill(&self) {
        docker(&["kill", &self.name]);
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

pub fn resolved(engine: Engine, port: u16, user: &str, pw: &str, db: Option<&str>) -> Resolved {
    Resolved { engine, host: "127.0.0.1".into(), port, user: user.into(), password: Some(pw.into()), database: db.map(String::from) }
}

pub fn session(r: &Resolved) -> Result<Session, DbError> {
    assert_eq!(r.host, "127.0.0.1", "integration tests only talk to local throwaway containers");
    Ok(Session::new(sql::connect(r)?))
}

pub fn run(s: &mut Session, q: &str) -> QueryReport {
    run_limit(s, q, 10_000)
}

pub fn run_limit(s: &mut Session, q: &str, limit: usize) -> QueryReport {
    s.run(q, limit).unwrap();
    s.wait(Duration::from_secs(60)).expect("query did not finish within 60s")
}

/// Append `engine|code|expected|line|message` to $KURO_RECORD_FIXTURES (used to refresh tests/fixtures/db_errors.txt).
pub fn record(engine: &str, e: &DbError) {
    if let Ok(path) = std::env::var("KURO_RECORD_FIXTURES") {
        use std::io::Write;
        let cat = match e.category {
            sql::ErrorCategory::Syntax => "syntax",
            sql::ErrorCategory::Runtime => "runtime",
            sql::ErrorCategory::Execution => "execution",
        };
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        let line = e.line.map(|l| l.to_string()).unwrap_or_default();
        writeln!(f, "{engine}|{}|{cat}|{line}|{}", e.code.clone().unwrap_or_default(), e.message.replace('\n', " ")).unwrap();
    }
}

pub fn err(engine: &str, r: QueryReport) -> DbError {
    let e = r.result.expect_err("expected an error");
    record(engine, &e);
    e
}

/// Poll `cond` until it is true, failing after a timeout. Tests use this instead of fixed sleeps.
pub fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let t = Instant::now();
    while !cond() {
        assert!(t.elapsed() < Duration::from_secs(30), "timed out waiting for: {what}");
        std::thread::yield_now();
    }
}
