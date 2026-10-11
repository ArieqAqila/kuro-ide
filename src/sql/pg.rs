use super::classify::{classify_pg, line_col_from_offset};
use super::profile::{Engine, Resolved};
use super::{Canceller, DbError, Driver, ResultSet};
use crate::table::Column;
use postgres::error::ErrorPosition;
use postgres::{Client, Config, NoTls, SimpleQueryMessage};
use std::sync::Arc;
use std::time::Duration;

pub struct PgDriver {
    client: Client,
    label: String,
}

/// True when `sql` holds exactly one statement (ignores `;` in quotes/comments).
pub fn is_single_statement(sql: &str) -> bool {
    let chars: Vec<char> = sql.chars().collect();
    let (mut i, mut stmts, mut pending) = (0, 0, false);
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' | '"' => {
                pending = true;
                i += 1;
                while i < chars.len() && chars[i] != c {
                    i += 1;
                }
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 1;
            }
            ';' => {
                if pending {
                    stmts += 1;
                }
                pending = false;
            }
            c if !c.is_whitespace() => pending = true,
            _ => {}
        }
        i += 1;
    }
    stmts + usize::from(pending) == 1
}

pub fn pg_error(e: &postgres::Error, sql: &str) -> DbError {
    if let Some(db) = e.as_db_error() {
        let (line, column) = match db.position() {
            Some(ErrorPosition::Original(n)) => {
                let (l, c) = line_col_from_offset(sql, *n as usize);
                (Some(l), Some(c))
            }
            _ => (None, None),
        };
        let mut message = db.message().to_string();
        if let Some(h) = db.hint() {
            message.push_str(&format!(" (hint: {h})"));
        }
        return DbError { category: classify_pg(db.code().code()), message, line, column, code: Some(db.code().code().to_string()) };
    }
    // not a server error: I/O failure, closed connection, timeout...
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        msg.push_str(&format!(": {s}"));
        src = s.source();
    }
    let prefix = if e.is_closed() { "connection lost: " } else { "" };
    DbError::execution(format!("{prefix}{msg}"))
}

impl PgDriver {
    pub fn connect(r: &Resolved) -> Result<Self, DbError> {
        let mut cfg = Config::new();
        cfg.host(&r.host).port(r.port).user(&r.user);
        if let Some(p) = &r.password {
            cfg.password(p);
        }
        if let Some(d) = &r.database {
            cfg.dbname(d);
        }
        cfg.connect_timeout(Duration::from_secs(10)).application_name("kuro");
        let client = cfg.connect(NoTls).map_err(|e| pg_error(&e, ""))?;
        let label = format!("postgres://{}@{}:{}/{}", r.user, r.host, r.port, r.database.clone().unwrap_or_default());
        Ok(PgDriver { client, label })
    }
}

impl Driver for PgDriver {
    fn execute(&mut self, sql: &str, row_limit: usize) -> Result<ResultSet, DbError> {
        // Column types are only available from a prepared statement. Preparing a
        // multi-statement script is an error, so only do it for single statements.
        let typed: Option<Vec<Column>> = if is_single_statement(sql) {
            let stmt = self.client.prepare(sql).map_err(|e| pg_error(&e, sql))?;
            Some(stmt.columns().iter().map(|c| Column { name: c.name().into(), type_name: c.type_().name().into() }).collect())
        } else {
            None
        };

        let msgs = self.client.simple_query(sql).map_err(|e| pg_error(&e, sql))?;

        let mut result: Option<ResultSet> = None;
        let mut cur = ResultSet::default();
        for m in msgs {
            match m {
                SimpleQueryMessage::Row(row) => {
                    if cur.columns.is_empty() {
                        cur.columns = row.columns().iter().map(|c| Column { name: c.name().into(), type_name: String::new() }).collect();
                    }
                    cur.total_rows += 1;
                    if cur.rows.len() < row_limit {
                        cur.rows.push((0..row.len()).map(|i| row.get(i).unwrap_or("NULL").to_string()).collect());
                    } else {
                        cur.truncated = true;
                    }
                }
                SimpleQueryMessage::CommandComplete(n) => {
                    if cur.columns.is_empty() {
                        cur.affected = Some(n);
                    }
                    // keep the last statement that produced columns, else the last one
                    if !cur.columns.is_empty() || result.as_ref().is_none_or(|r| r.columns.is_empty()) {
                        result = Some(std::mem::take(&mut cur));
                    } else {
                        cur = ResultSet::default();
                    }
                }
                _ => {}
            }
        }
        let mut rs = result.unwrap_or_default();
        if let Some(types) = typed {
            if rs.columns.is_empty() && !types.is_empty() {
                // zero-row SELECT: still show headers
                rs.columns = types;
                rs.affected = None;
            } else if types.len() == rs.columns.len() {
                for (c, t) in rs.columns.iter_mut().zip(types) {
                    c.type_name = t.type_name;
                }
            }
        }
        Ok(rs)
    }

    fn canceller(&self) -> Canceller {
        let token = self.client.cancel_token();
        Arc::new(move || token.cancel_query(NoTls).map_err(|e| e.to_string()))
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn engine(&self) -> Engine {
        Engine::Postgres
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_statement_detection() {
        assert!(is_single_statement("select 1"));
        assert!(is_single_statement("select 1;"));
        assert!(is_single_statement("  select ';' ; -- trailing; comment\n"));
        assert!(is_single_statement("select /* a;b */ 1"));
        assert!(!is_single_statement("select 1; select 2"));
        assert!(!is_single_statement("select 1;\nselect 2;"));
        assert!(!is_single_statement(""));
        assert!(!is_single_statement("-- only a comment"));
    }
}
