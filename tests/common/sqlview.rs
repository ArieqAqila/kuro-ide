//! Helpers to put canned SQL results into the app without a database.
#![allow(dead_code)]

use kuro::app::{App, ResultView};
use kuro::sql::{self, DbError, ErrorCategory, QueryReport, ResultSet};
use kuro::table::Column;
use std::time::Duration;

pub fn rows_result(nrows: usize, ncols: usize) -> ResultSet {
    let columns: Vec<Column> = (0..ncols).map(|c| Column { name: format!("col{c}"), type_name: "text".into() }).collect();
    let rows: Vec<Vec<String>> = (0..nrows).map(|r| (0..ncols).map(|c| format!("r{r}c{c}-{}", "x".repeat(c * 3))).collect()).collect();
    ResultSet { columns, rows, truncated: false, total_rows: nrows, affected: None, ..Default::default() }
}

pub fn show(app: &mut App, rs: ResultSet) {
    let rep = QueryReport { elapsed: Duration::from_millis(7), result: Ok(rs) };
    let status = sql::status_text(&rep);
    app.sql.result = Some(ResultView::new(rep.result, rep.elapsed, status));
    app.sql.pane_open = true;
}

pub fn show_rows(app: &mut App, nrows: usize, ncols: usize) {
    show(app, rows_result(nrows, ncols));
}

pub fn show_error(app: &mut App, cat: &str, msg: &str, line: Option<usize>, col: Option<usize>) {
    let e = DbError { category: ErrorCategory::parse(cat).unwrap(), message: msg.into(), line, column: col, code: None };
    let rep = QueryReport { elapsed: Duration::from_millis(3), result: Err(e) };
    let status = sql::status_text(&rep);
    app.sql.result = Some(ResultView::new(rep.result, rep.elapsed, status));
    app.sql.pane_open = true;
}
