//! Classification of recorded database error messages (no database needed).

use kuro::sql::classify::*;

struct Row {
    engine: String,
    code: String,
    expected: ErrorCategory,
    message: String,
}

fn rows() -> Vec<Row> {
    include_str!("fixtures/db_errors.txt")
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut p = l.splitn(4, '|');
            let (engine, code, cat, message) = (p.next().unwrap(), p.next().unwrap(), p.next().unwrap(), p.next().unwrap());
            Row {
                engine: engine.into(),
                code: code.into(),
                expected: ErrorCategory::parse(cat).unwrap_or_else(|| panic!("bad category in fixture: {l}")),
                message: message.into(),
            }
        })
        .collect()
}

#[test]
fn fixtures_cover_all_categories_for_both_engines() {
    let rows = rows();
    for engine in ["postgres", "mariadb"] {
        for cat in [ErrorCategory::Syntax, ErrorCategory::Runtime, ErrorCategory::Execution] {
            assert!(rows.iter().any(|r| r.engine == engine && r.expected == cat), "no {engine} fixture for {cat:?}");
        }
    }
}

#[test]
fn recorded_errors_classify_as_expected() {
    for r in rows() {
        // fixtures without a code are client-side failures (always execution errors)
        let got = match (r.engine.as_str(), r.code.as_str()) {
            (_, "") => ErrorCategory::Execution,
            ("postgres", c) => classify_pg(c),
            ("mariadb", c) => classify_mysql(c.parse().unwrap(), ""),
            (e, _) => panic!("unknown engine {e}"),
        };
        assert_eq!(got, r.expected, "{} {} {:?}", r.engine, r.code, r.message);
    }
}

#[test]
fn mariadb_syntax_messages_carry_a_line() {
    for r in rows().iter().filter(|r| r.engine == "mariadb" && r.expected == ErrorCategory::Syntax) {
        assert!(mysql_line(&r.message).is_some(), "{}", r.message);
        assert!(mysql_position("SELECT 1;\nSELEC 1", &r.message).0.is_some());
    }
}
