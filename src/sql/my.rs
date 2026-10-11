use super::classify::{classify_mysql, mysql_position};
use super::profile::Resolved;
use super::{Canceller, DbError, Driver, ResultSet};
use crate::table::Column;
use ::mysql::prelude::Queryable;
use ::mysql::{Conn, Opts, OptsBuilder, Value};
use std::sync::Arc;
use std::time::Duration;

pub struct MyDriver {
    conn: Conn,
    opts: Opts,
    label: String,
}

pub fn my_error(e: &::mysql::Error, sql: &str) -> DbError {
    match e {
        ::mysql::Error::MySqlError(m) => {
            let (line, column) = mysql_position(sql, &m.message);
            DbError { category: classify_mysql(m.code, &m.state), message: m.message.clone(), line, column, code: Some(m.code.to_string()) }
        }
        ::mysql::Error::IoError(_) | ::mysql::Error::CodecError(_) => DbError::execution(format!("connection lost: {e}")),
        other => DbError::execution(other.to_string()),
    }
}

fn value_to_string(v: Value) -> String {
    match v {
        Value::NULL => "NULL".into(),
        Value::Bytes(b) => String::from_utf8_lossy(&b).into_owned(),
        other => {
            let s = other.as_sql(true);
            s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')).map(str::to_string).unwrap_or(s)
        }
    }
}

fn type_name(t: ::mysql::consts::ColumnType) -> String {
    format!("{t:?}").trim_start_matches("MYSQL_TYPE_").to_lowercase()
}

impl MyDriver {
    pub fn connect(r: &Resolved) -> Result<Self, DbError> {
        let opts: Opts = OptsBuilder::new()
            .ip_or_hostname(Some(r.host.clone()))
            .tcp_port(r.port)
            .user(Some(r.user.clone()))
            .pass(r.password.clone())
            .db_name(r.database.clone())
            .tcp_connect_timeout(Some(Duration::from_secs(10)))
            .into();
        let conn = Conn::new(opts.clone()).map_err(|e| my_error(&e, ""))?;
        let label = format!("mariadb://{}@{}:{}/{}", r.user, r.host, r.port, r.database.clone().unwrap_or_default());
        Ok(MyDriver { conn, opts, label })
    }
}

impl Driver for MyDriver {
    fn execute(&mut self, sql: &str, row_limit: usize) -> Result<ResultSet, DbError> {
        let mut res = self.conn.query_iter(sql).map_err(|e| my_error(&e, sql))?;
        let mut out: Option<ResultSet> = None;
        while let Some(set) = res.iter() {
            let columns: Vec<Column> = set
                .columns()
                .as_ref()
                .iter()
                .map(|c| Column { name: c.name_str().to_string(), type_name: type_name(c.column_type()) })
                .collect();
            let affected = set.affected_rows();
            let mut cur = ResultSet { columns, ..Default::default() };
            if cur.columns.is_empty() {
                cur.affected = Some(affected);
            }
            for row in set {
                let row = row.map_err(|e| my_error(&e, sql))?;
                cur.total_rows += 1;
                if cur.rows.len() < row_limit {
                    cur.rows.push(row.unwrap().into_iter().map(value_to_string).collect());
                } else {
                    cur.truncated = true;
                }
            }
            // keep the last set that has columns, else the last one
            if !cur.columns.is_empty() || out.as_ref().is_none_or(|o| o.columns.is_empty()) {
                out = Some(cur);
            }
        }
        Ok(out.unwrap_or_default())
    }

    fn canceller(&self) -> Canceller {
        let opts = self.opts.clone();
        let id = self.conn.connection_id();
        Arc::new(move || {
            // KILL QUERY must come from a second connection
            let mut c = Conn::new(opts.clone()).map_err(|e| e.to_string())?;
            c.query_drop(format!("KILL QUERY {id}")).map_err(|e| e.to_string())
        })
    }

    fn label(&self) -> String {
        self.label.clone()
    }
}
