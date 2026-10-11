//! Integration tests against real MariaDB and PostgreSQL in throwaway Docker
//! containers (bound to 127.0.0.1). Skipped cleanly when Docker is unavailable.

mod common;
use common::*;
use kuro::sql::{ErrorCategory::*, profile::Engine};
use std::time::Duration;

#[test]
fn postgres() {
    if docker_unavailable().is_some() {
        return;
    }
    let c = Container::start("pg", "postgres:16-alpine", 5432, &[("POSTGRES_PASSWORD", "{pw}")]);
    c.wait_log("database system is ready to accept connections", 2);
    let su = resolved(Engine::Postgres, c.port, "postgres", &c.password, Some("postgres"));
    let mut s = retry_connect(&su);

    // connect + run a query with typed columns
    let rs = run(&mut s, "SELECT 1 AS n, 'x'::text AS t, NULL::int AS z").result.unwrap();
    assert_eq!(
        rs.columns.iter().map(|c| (c.name.as_str(), c.type_name.as_str())).collect::<Vec<_>>(),
        [("n", "int4"), ("t", "text"), ("z", "int4")]
    );
    assert_eq!(rs.rows, vec![vec!["1", "x", "NULL"]]);

    // zero rows still carries headers
    let rs = run(&mut s, "SELECT 1 AS n WHERE false").result.unwrap();
    assert_eq!((rs.columns.len(), rs.rows.len()), (1, 0));

    // DDL / DML
    run(&mut s, "CREATE TABLE t (id int PRIMARY KEY, n int NOT NULL)").result.unwrap();
    let rs = run(&mut s, "INSERT INTO t VALUES (1,1),(2,2),(3,3)").result.unwrap();
    assert_eq!(rs.affected, Some(3));

    // syntax error, with line and column
    let e = err("postgres", run(&mut s, "SELEC 1"));
    assert_eq!((e.category, e.line, e.column, e.code.as_deref()), (Syntax, Some(1), Some(1), Some("42601")), "{e}");
    let e = err("postgres", run(&mut s, "SELECT 1;\nSELECT FROM WHERE;"));
    assert_eq!(e.category, Syntax);
    assert_eq!(e.line, Some(2), "{e}");

    // runtime errors: division by zero, bad cast (type mismatch), constraint violation
    let e = err("postgres", run(&mut s, "SELECT 1/0"));
    assert_eq!((e.category, e.code.as_deref()), (Runtime, Some("22012")));
    let e = err("postgres", run(&mut s, "SELECT 'abc'::int"));
    assert_eq!((e.category, e.code.as_deref()), (Runtime, Some("22P02")));
    let e = err("postgres", run(&mut s, "INSERT INTO t VALUES (1, 1)"));
    assert_eq!((e.category, e.code.as_deref()), (Runtime, Some("23505")));
    let e = err("postgres", run(&mut s, "SELECT * FROM t WHERE id = 'x'"));
    assert_eq!(e.category, Runtime);

    // execution errors: missing table, missing column, permission denied
    let e = err("postgres", run(&mut s, "SELECT * FROM no_such_table"));
    assert_eq!((e.category, e.code.as_deref()), (Execution, Some("42P01")));
    let e = err("postgres", run(&mut s, "SELECT nope FROM t"));
    assert_eq!((e.category, e.code.as_deref()), (Execution, Some("42703")));
    run(&mut s, "CREATE ROLE limited LOGIN PASSWORD 'limited_pw'; CREATE TABLE secret (x int)").result.unwrap();
    let lim = resolved(Engine::Postgres, c.port, "limited", "limited_pw", Some("postgres"));
    let mut ls = session(&lim).unwrap();
    let e = err("postgres", run(&mut ls, "SELECT * FROM secret"));
    assert_eq!((e.category, e.code.as_deref()), (Execution, Some("42501")), "{e}");

    // the session survives errors
    assert!(run(&mut s, "SELECT 1").result.is_ok());

    // wrong password is an execution error
    let bad = resolved(Engine::Postgres, c.port, "postgres", "wrong", Some("postgres"));
    let e = session(&bad).err().expect("connect must fail");
    record("postgres", &e);
    assert_eq!((e.category, e.code.as_deref()), (Execution, Some("28P01")), "{e}");

    // result set larger than row_limit
    let r = run_limit(&mut s, "SELECT g, md5(g::text) FROM generate_series(1,200000) g", 1000);
    let rs = r.result.unwrap();
    assert_eq!((rs.rows.len(), rs.total_rows, rs.truncated), (1000, 200_000, true));
    assert!(r.elapsed < Duration::from_secs(30));

    // long-running query + cancel
    s.run("SELECT pg_sleep(30)", 100).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    s.cancel().unwrap();
    let r = s.wait(Duration::from_secs(10)).expect("cancel did not stop the query");
    assert!(r.elapsed < Duration::from_secs(10));
    let e = err("postgres", r);
    assert_eq!((e.category, e.code.as_deref()), (Execution, Some("57014")), "{e}");
    assert!(run(&mut s, "SELECT 1").result.is_ok(), "session usable after cancel");

    // connection lost
    c.kill();
    let e = err("postgres", run(&mut s, "SELECT 1"));
    assert_eq!(e.category, Execution, "{e}");
}

#[test]
fn mariadb() {
    if docker_unavailable().is_some() {
        return;
    }
    let c = Container::start("maria", "mariadb:11", 3306, &[("MARIADB_ROOT_PASSWORD", "{pw}"), ("MARIADB_DATABASE", "test")]);
    c.wait_log("ready for connections", 2);
    let su = resolved(Engine::MariaDb, c.port, "root", &c.password, Some("test"));
    let mut s = retry_connect(&su);

    let rs = run(&mut s, "SELECT 1 AS n, 'x' AS t, NULL AS z").result.unwrap();
    assert_eq!(rs.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["n", "t", "z"]);
    assert_eq!(rs.rows, vec![vec!["1", "x", "NULL"]]);
    assert!(!rs.columns[0].type_name.is_empty(), "types are shown");

    let rs = run(&mut s, "SELECT 1 AS n WHERE 1=0").result.unwrap();
    assert_eq!((rs.columns.len(), rs.rows.len()), (1, 0));

    run(&mut s, "CREATE TABLE t (id int PRIMARY KEY, n int NOT NULL)").result.unwrap();
    let rs = run(&mut s, "INSERT INTO t VALUES (1,1),(2,2),(3,3)").result.unwrap();
    assert_eq!(rs.affected, Some(3));

    // syntax error with line number
    let e = err("mariadb", run(&mut s, "SELEC 1"));
    assert_eq!((e.category, e.line, e.code.as_deref()), (Syntax, Some(1), Some("1064")), "{e}");
    let e = err("mariadb", run(&mut s, "SELECT 1;\nSELECT FROM WHERE;"));
    assert_eq!(e.category, Syntax);
    assert_eq!(e.line, Some(2), "{e}");

    // runtime: type mismatch on insert, duplicate key, NULL into NOT NULL
    let e = err("mariadb", run(&mut s, "INSERT INTO t VALUES ('abc', 1)"));
    assert_eq!((e.category, e.code.as_deref()), (Runtime, Some("1366")), "{e}");
    let e = err("mariadb", run(&mut s, "INSERT INTO t VALUES (1, 1)"));
    assert_eq!((e.category, e.code.as_deref()), (Runtime, Some("1062")));
    let e = err("mariadb", run(&mut s, "INSERT INTO t VALUES (9, NULL)"));
    assert_eq!((e.category, e.code.as_deref()), (Runtime, Some("1048")));

    // execution: missing table / column / permission denied
    let e = err("mariadb", run(&mut s, "SELECT * FROM no_such_table"));
    assert_eq!((e.category, e.code.as_deref()), (Execution, Some("1146")));
    let e = err("mariadb", run(&mut s, "SELECT nope FROM t"));
    assert_eq!((e.category, e.code.as_deref()), (Execution, Some("1054")));
    run(&mut s, "CREATE USER 'limited'@'%' IDENTIFIED BY 'limited_pw'").result.unwrap();
    let lim = resolved(Engine::MariaDb, c.port, "limited", "limited_pw", None);
    let mut ls = session(&lim).unwrap();
    let e = err("mariadb", run(&mut ls, "SELECT * FROM test.t"));
    assert_eq!((e.category, e.code.as_deref()), (Execution, Some("1142")), "{e}");

    assert!(run(&mut s, "SELECT 1").result.is_ok(), "session survives errors");

    let bad = resolved(Engine::MariaDb, c.port, "root", "wrong", Some("test"));
    let e = session(&bad).err().expect("connect must fail");
    record("mariadb", &e);
    assert_eq!((e.category, e.code.as_deref()), (Execution, Some("1045")), "{e}");

    let r = run_limit(&mut s, "SELECT seq, md5(seq) FROM seq_1_to_200000", 1000);
    let rs = r.result.unwrap();
    assert_eq!((rs.rows.len(), rs.total_rows, rs.truncated), (1000, 200_000, true));

    // long-running + cancel (MariaDB may end it with an error or an interrupted result)
    s.run("SELECT SLEEP(30)", 100).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    s.cancel().unwrap();
    let r = s.wait(Duration::from_secs(10)).expect("cancel did not stop the query");
    assert!(r.elapsed < Duration::from_secs(10), "cancelled after {:?}", r.elapsed);
    if let Err(e) = &r.result {
        record("mariadb", e);
        assert_eq!(e.category, Execution);
    }
    assert!(run(&mut s, "SELECT 1").result.is_ok(), "session usable after cancel");

    c.kill();
    let e = err("mariadb", run(&mut s, "SELECT 1"));
    assert_eq!(e.category, Execution, "{e}");
}

fn retry_connect(r: &kuro::sql::profile::Resolved) -> kuro::sql::Session {
    let t = std::time::Instant::now();
    loop {
        match session(r) {
            Ok(s) => return s,
            Err(e) if t.elapsed() > Duration::from_secs(90) => panic!("could not connect to test container: {e}"),
            Err(_) => std::thread::sleep(Duration::from_secs(1)),
        }
    }
}
