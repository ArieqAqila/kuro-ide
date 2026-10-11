//! SQL snippets per database driver. Placeholders look like `<table>`; they are inserted as plain text
//! for you to replace. The integration tests run every snippet against real MariaDB and PostgreSQL
//! (with the placeholders substituted), so none of them is merely plausible.
//!
//! PostgreSQL note: `\dt`, `\d table` and friends are psql client commands, not SQL: the server rejects
//! them, so the PostgreSQL snippets use `information_schema` / `pg_catalog` instead.

use crate::sql::profile::Engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snippet {
    pub name: &'static str,
    pub description: &'static str,
    pub body: &'static str,
}

const fn s(name: &'static str, description: &'static str, body: &'static str) -> Snippet {
    Snippet { name, description, body }
}

/// Portable SQL, shown when there is no connection.
pub const GENERIC: &[Snippet] = &[
    s("select", "select rows", "SELECT <columns>\nFROM <table>\nWHERE <condition>;\n"),
    s("select limit", "first N rows (LIMIT/OFFSET)", "SELECT *\nFROM <table>\nLIMIT 10 OFFSET 0;\n"),
    s("insert", "insert a row", "INSERT INTO <table> (<column>)\nVALUES (<value>);\n"),
    s("update", "update rows", "UPDATE <table>\nSET <column> = <value>\nWHERE <condition>;\n"),
    s("delete", "delete rows", "DELETE FROM <table>\nWHERE <condition>;\n"),
    s("join", "inner join", "SELECT a.*, b.*\nFROM <table> a\nJOIN <other_table> b ON b.<column> = a.<column>;\n"),
    s("group by", "aggregate", "SELECT <column>, COUNT(*) AS n\nFROM <table>\nGROUP BY <column>\nORDER BY n DESC;\n"),
    s("create table", "new table", "CREATE TABLE <table> (\n\tid INTEGER PRIMARY KEY,\n\tname VARCHAR(100) NOT NULL\n);\n"),
    s("drop table", "remove a table", "DROP TABLE <table>;\n"),
];

pub const MARIADB: &[Snippet] = &[
    s("show databases", "list databases", "SHOW DATABASES;\n"),
    s("show tables", "list tables in the current database", "SHOW TABLES;\n"),
    s("describe", "columns of a table (DESCRIBE)", "DESCRIBE <table>;\n"),
    s("show create table", "DDL of a table", "SHOW CREATE TABLE <table>;\n"),
    s("show columns", "columns with details", "SHOW FULL COLUMNS FROM <table>;\n"),
    s(
        "information_schema tables",
        "tables via information_schema",
        "SELECT table_name, table_rows\nFROM information_schema.tables\nWHERE table_schema = DATABASE();\n",
    ),
    s("limit offset", "paging: LIMIT count OFFSET skip", "SELECT *\nFROM <table>\nLIMIT 10 OFFSET 20;\n"),
    s("limit short", "paging: LIMIT skip, count (MariaDB/MySQL form)", "SELECT *\nFROM <table>\nLIMIT 20, 10;\n"),
    s("now", "current date and time", "SELECT NOW(), CURDATE(), CURTIME();\n"),
    s("date add", "date arithmetic", "SELECT DATE_ADD(NOW(), INTERVAL 1 DAY), DATE_SUB(CURDATE(), INTERVAL 1 MONTH);\n"),
    s("date format", "format a date", "SELECT DATE_FORMAT(NOW(), '%Y-%m-%d %H:%i:%s');\n"),
    s(
        "upsert",
        "insert or update on duplicate key",
        "INSERT INTO <table> (id, <column>)\nVALUES (1, <value>)\nON DUPLICATE KEY UPDATE <column> = VALUES(<column>);\n",
    ),
    s(
        "create table",
        "table with AUTO_INCREMENT",
        "CREATE TABLE <table> (\n\tid INT AUTO_INCREMENT PRIMARY KEY,\n\tname VARCHAR(100) NOT NULL,\n\tcreated_at DATETIME DEFAULT CURRENT_TIMESTAMP\n);\n",
    ),
    s("explain", "query plan", "EXPLAIN SELECT * FROM <table>;\n"),
    s("processlist", "running queries", "SHOW FULL PROCESSLIST;\n"),
    s("show warnings", "warnings of the last statement", "SHOW WARNINGS;\n"),
    s("version", "server version", "SELECT VERSION();\n"),
    s("variables", "server variables", "SHOW VARIABLES LIKE '%timeout%';\n"),
    s("transaction", "transaction block", "START TRANSACTION;\n<statements>\nCOMMIT;\n"),
];

pub const POSTGRES: &[Snippet] = &[
    s("list schemas", "schemas (psql: \\dn)", "SELECT schema_name\nFROM information_schema.schemata\nORDER BY schema_name;\n"),
    s(
        "list tables",
        "tables (psql: \\dt)",
        "SELECT table_schema, table_name\nFROM information_schema.tables\nWHERE table_schema NOT IN ('pg_catalog', 'information_schema')\nORDER BY 1, 2;\n",
    ),
    s(
        "describe table",
        "columns of a table (psql: \\d table)",
        "SELECT column_name, data_type, is_nullable, column_default\nFROM information_schema.columns\nWHERE table_name = '<table>'\nORDER BY ordinal_position;\n",
    ),
    s(
        "table sizes",
        "size on disk per table",
        "SELECT relname, pg_size_pretty(pg_total_relation_size(oid)) AS size\nFROM pg_class\nWHERE relkind = 'r' AND relnamespace = 'public'::regnamespace\nORDER BY pg_total_relation_size(oid) DESC;\n",
    ),
    s("limit offset", "paging: LIMIT count OFFSET skip", "SELECT *\nFROM <table>\nLIMIT 10 OFFSET 20;\n"),
    s("fetch first", "paging: standard FETCH FIRST", "SELECT *\nFROM <table>\nORDER BY 1\nOFFSET 20 ROWS FETCH FIRST 10 ROWS ONLY;\n"),
    s("now", "current date and time", "SELECT now(), current_date, current_time;\n"),
    s("date arithmetic", "intervals", "SELECT now() + interval '1 day', current_date - interval '1 month';\n"),
    s("to_char", "format a date", "SELECT to_char(now(), 'YYYY-MM-DD HH24:MI:SS');\n"),
    s("date_trunc", "truncate a timestamp", "SELECT date_trunc('hour', now());\n"),
    s(
        "upsert",
        "insert or update on conflict",
        "INSERT INTO <table> (id, <column>)\nVALUES (1, <value>)\nON CONFLICT (id) DO UPDATE SET <column> = EXCLUDED.<column>;\n",
    ),
    s(
        "create table",
        "table with identity column",
        "CREATE TABLE <table> (\n\tid INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\n\tname VARCHAR(100) NOT NULL,\n\tcreated_at TIMESTAMPTZ DEFAULT now()\n);\n",
    ),
    s("explain analyze", "query plan with timings", "EXPLAIN ANALYZE SELECT * FROM <table>;\n"),
    s("activity", "running queries", "SELECT pid, state, query\nFROM pg_stat_activity\nWHERE datname = current_database();\n"),
    s("generate_series", "generate rows", "SELECT g FROM generate_series(1, 10) AS g;\n"),
    s("jsonb", "query a jsonb column", "SELECT <column> ->> 'key' AS value\nFROM <table>\nWHERE <column> @> '{\"key\": \"value\"}';\n"),
    s("version", "server version", "SELECT version();\n"),
    s("transaction", "transaction block", "BEGIN;\n<statements>\nCOMMIT;\n"),
];

/// Snippets for the connected driver, or the generic list (second value `true`) with no connection.
pub fn for_engine(engine: Option<Engine>) -> (&'static [Snippet], bool) {
    match engine {
        Some(Engine::MariaDb) => (MARIADB, false),
        Some(Engine::Postgres) => (POSTGRES, false),
        None => (GENERIC, true),
    }
}

/// Fill `<placeholder>` markers with real names (used by the integration tests to execute snippets).
pub fn fill(body: &str, table: &str, other: &str, column: &str, value: &str, statements: &str) -> String {
    body.replace("<other_table>", other)
        .replace("<table>", table)
        .replace("<columns>", column)
        .replace("<column>", column)
        .replace("<condition>", "1 = 1")
        .replace("<value>", value)
        .replace("<statements>", statements)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_driver_has_its_own_distinct_set() {
        let (m, mg) = for_engine(Some(Engine::MariaDb));
        let (p, pg) = for_engine(Some(Engine::Postgres));
        let (g, gg) = for_engine(None);
        assert!(!mg && !pg && gg, "only the no-connection list is flagged generic");
        assert_ne!(m, p);
        assert_ne!(m, g);
        assert_ne!(p, g);
    }

    #[test]
    fn mariadb_and_postgres_differ_where_the_dialects_differ() {
        let has = |set: &[Snippet], needle: &str| set.iter().any(|s| s.body.contains(needle));
        assert!(has(MARIADB, "SHOW TABLES") && !has(POSTGRES, "SHOW TABLES"));
        assert!(has(MARIADB, "DESCRIBE") && !has(POSTGRES, "DESCRIBE"));
        assert!(has(POSTGRES, "information_schema") && has(MARIADB, "information_schema"));
        assert!(has(MARIADB, "LIMIT 20, 10"), "MariaDB short LIMIT form");
        assert!(has(POSTGRES, "FETCH FIRST"));
        assert!(has(MARIADB, "DATE_ADD") && has(POSTGRES, "interval '1 day'"));
        assert!(has(MARIADB, "ON DUPLICATE KEY") && has(POSTGRES, "ON CONFLICT"));
        assert!(!has(POSTGRES, "\\dt\n") && POSTGRES.iter().all(|s| !s.body.starts_with('\\')), "psql meta-commands are not SQL");
    }

    #[test]
    fn names_are_unique_per_list_and_bodies_end_with_a_newline() {
        for set in [GENERIC, MARIADB, POSTGRES] {
            let mut names: Vec<_> = set.iter().map(|s| s.name).collect();
            names.sort();
            let n = names.len();
            names.dedup();
            assert_eq!(names.len(), n, "duplicate snippet name");
            assert!(set.iter().all(|s| s.body.ends_with('\n') && !s.description.is_empty()));
        }
    }

    #[test]
    fn fill_replaces_every_placeholder() {
        for set in [GENERIC, MARIADB, POSTGRES] {
            for sn in set {
                let f = fill(sn.body, "t", "u", "id", "1", "SELECT 1;");
                assert!(!f.contains('<') || f.contains("<>") || f.contains("<="), "{}: {f}", sn.name);
            }
        }
    }
}
