//! Error classification for MariaDB and PostgreSQL.
//!
//! Categories:
//! * **Syntax**    – the server could not parse the statement.
//! * **Runtime**   – the statement parsed but failed on data/types
//!   (division by zero, bad cast, constraint violation, type mismatch).
//! * **Execution** – everything about the environment or objects: missing
//!   table/column, permission denied, connection lost,
//!   timeout, cancelled. This is also the fallback for unknown codes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Syntax,
    Runtime,
    Execution,
}

impl ErrorCategory {
    pub fn label(self) -> &'static str {
        match self {
            ErrorCategory::Syntax => "syntax error",
            ErrorCategory::Runtime => "runtime error",
            ErrorCategory::Execution => "execution error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "syntax" => Some(Self::Syntax),
            "runtime" => Some(Self::Runtime),
            "execution" => Some(Self::Execution),
            _ => None,
        }
    }
}

/// Classify a PostgreSQL SQLSTATE.
pub fn classify_pg(sqlstate: &str) -> ErrorCategory {
    use ErrorCategory::*;
    match sqlstate {
        // syntax_error, invalid_name, invalid_column_definition, name_too_long, reserved_name
        "42601" | "42602" | "42611" | "42622" | "42939" | "42000" => Syntax,
        // datatype_mismatch, cannot_coerce, undefined_function (e.g. `int = text`: no such operator),
        // cardinality_violation, serialization failure, deadlock, raise_exception
        "42804" | "42846" | "42883" | "21000" | "40001" | "40P01" | "P0001" => Runtime,
        _ => match sqlstate.get(..2) {
            // data_exception (22xxx), integrity_constraint_violation (23xxx)
            Some("22") | Some("23") => Runtime,
            // everything else, e.g. 42P01 undefined_table, 42501 insufficient_privilege,
            // 08 connection, 28 auth, 57 operator intervention (57014 cancel)
            _ => Execution,
        },
    }
}

/// Classify a MySQL/MariaDB error by vendor code (SQLSTATE is only a fallback,
/// because 42000 is shared by syntax errors and access-denied errors).
pub fn classify_mysql(code: u16, sqlstate: &str) -> ErrorCategory {
    use ErrorCategory::*;
    match code {
        // ER_PARSE_ERROR, ER_SYNTAX_ERROR, ER_EMPTY_QUERY
        1064 | 1149 | 1065 => Syntax,
        // NULL violation, duplicate key, column count mismatch, out of range, truncated data,
        // incorrect value, division by zero, data too long, FK violations, illegal collation mix,
        // subquery returns >1 row, BIGINT out of range, deadlock
        1048 | 1062 | 1136 | 1264 | 1265 | 1292 | 1366 | 1365 | 1406 | 1451 | 1452 | 1267 | 1242 | 1690 | 1367 | 1525 | 1213 | 4025 => {
            Runtime
        }
        _ => match sqlstate.get(..2) {
            Some("22") | Some("23") => Runtime,
            _ => Execution,
        },
    }
}

/// 1-based char `offset` into `sql` -> 1-based (line, column).
pub fn line_col_from_offset(sql: &str, offset: usize) -> (usize, usize) {
    let (mut line, mut col) = (1, 1);
    for (i, c) in sql.chars().enumerate() {
        if i + 1 >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// MySQL/MariaDB parse errors end in `... near '...' at line N`.
pub fn mysql_line(message: &str) -> Option<usize> {
    let idx = message.rfind(" at line ")?;
    message[idx + " at line ".len()..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// MariaDB reports `near '<rest of statement>' at line N`, where N is relative to
/// the failing statement. Locate that text inside the full script to get an
/// absolute (line, column); falls back to the reported line.
pub fn mysql_position(sql: &str, message: &str) -> (Option<usize>, Option<usize>) {
    let reported = mysql_line(message);
    let Some(start) = message.rfind("near '") else { return (reported, None) };
    let Some(end) = message.rfind("' at line ") else { return (reported, None) };
    if end < start + 6 {
        return (reported, None);
    }
    let near = &message[start + 6..end];
    let byte = if near.is_empty() {
        sql.trim_end().len()
    } else {
        // first occurrence that is not above the reported (relative) line
        let mut from = 0;
        let mut found = None;
        while let Some(i) = sql[from..].find(near) {
            let at = from + i;
            let abs = sql[..at].matches('\n').count() + 1;
            if reported.is_none_or(|r| abs >= r) {
                found = Some(at);
                break;
            }
            from = at + near.len().max(1);
        }
        match found {
            Some(b) => b,
            None => return (reported, None),
        }
    };
    let (l, c) = line_col_from_offset(sql, sql[..byte].chars().count() + 1);
    (Some(l), Some(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pg_codes() {
        use ErrorCategory::*;
        assert_eq!(classify_pg("42601"), Syntax);
        assert_eq!(classify_pg("22012"), Runtime, "division by zero");
        assert_eq!(classify_pg("22P02"), Runtime, "invalid text representation");
        assert_eq!(classify_pg("23505"), Runtime, "unique violation");
        assert_eq!(classify_pg("42804"), Runtime, "datatype mismatch");
        assert_eq!(classify_pg("42883"), Runtime, "no operator for types");
        assert_eq!(classify_pg("42P01"), Execution, "undefined table");
        assert_eq!(classify_pg("42703"), Execution, "undefined column");
        assert_eq!(classify_pg("42501"), Execution, "permission denied");
        assert_eq!(classify_pg("57014"), Execution, "cancelled");
        assert_eq!(classify_pg("08006"), Execution, "connection failure");
        assert_eq!(classify_pg("28P01"), Execution, "bad password");
        assert_eq!(classify_pg("ZZZZZ"), Execution, "unknown falls back to execution");
        assert_eq!(classify_pg(""), Execution);
    }

    #[test]
    fn mysql_codes() {
        use ErrorCategory::*;
        assert_eq!(classify_mysql(1064, "42000"), Syntax);
        assert_eq!(classify_mysql(1366, "22007"), Runtime);
        assert_eq!(classify_mysql(1062, "23000"), Runtime);
        assert_eq!(classify_mysql(1146, "42S02"), Execution, "table doesn't exist");
        assert_eq!(classify_mysql(1054, "42S22"), Execution, "unknown column");
        assert_eq!(classify_mysql(1142, "42000"), Execution, "access denied shares 42000 with syntax");
        assert_eq!(classify_mysql(1044, "42000"), Execution);
        assert_eq!(classify_mysql(1317, "70100"), Execution, "interrupted");
        assert_eq!(classify_mysql(2013, "HY000"), Execution, "lost connection");
        assert_eq!(classify_mysql(9999, "22003"), Runtime, "sqlstate class fallback");
        assert_eq!(classify_mysql(9999, ""), Execution);
    }

    #[test]
    fn offsets_to_line_col() {
        let sql = "SELECT 1;\nSELEC 2";
        assert_eq!(line_col_from_offset(sql, 1), (1, 1));
        assert_eq!(line_col_from_offset(sql, 8), (1, 8));
        assert_eq!(line_col_from_offset(sql, 11), (2, 1));
        assert_eq!(line_col_from_offset(sql, 13), (2, 3));
        assert_eq!(line_col_from_offset("é€x", 3), (1, 3), "offsets count chars");
        assert_eq!(line_col_from_offset("abc", 99), (1, 4), "past the end clamps");
    }

    #[test]
    fn mysql_position_locates_near_text_in_the_script() {
        let msg = "You have an error in your SQL syntax; check the manual ... for the right syntax to use near 'FROM WHERE;' at line 1";
        assert_eq!(mysql_position("SELECT 1;\nSELECT FROM WHERE;", msg), (Some(2), Some(8)));
        assert_eq!(mysql_position("SELEC 1", "... near 'SELEC 1' at line 1"), (Some(1), Some(1)));
        // error at end of input
        assert_eq!(mysql_position("SELECT 1 +\n", "... near '' at line 1"), (Some(1), Some(11)));
        // text not found: keep the reported line, no column
        assert_eq!(mysql_position("x", "... near 'zzz' at line 4"), (Some(4), None));
        assert_eq!(mysql_position("x", "Unknown column"), (None, None));
    }

    #[test]
    fn mysql_line_parsing() {
        assert_eq!(mysql_line("You have an error in your SQL syntax; ... near 'SELEC 1' at line 3"), Some(3));
        assert_eq!(mysql_line("Unknown column 'x' in 'field list'"), None);
        assert_eq!(mysql_line("... near '' at line 12 trailing"), Some(12));
    }
}
