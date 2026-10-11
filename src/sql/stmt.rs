//! Splitting SQL text into statements, to find "the statement under the cursor".
//!
//! Handles `;` outside of single/double quotes, backticks, `--` and `/* */` comments, `#` comments (MariaDB),
//! and PostgreSQL dollar quoting (`$$ ... $$`, `$tag$ ... $tag$`). Backslash escapes inside strings are a
//! MariaDB behaviour (PostgreSQL's standard strings have none), so the caller says which applies.

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    /// `'it\'s'` escapes the quote (MariaDB) rather than ending the string (PostgreSQL).
    pub backslash_escapes: bool,
    /// `# comment` (MariaDB)
    pub hash_comments: bool,
    /// `$tag$ ... $tag$` (PostgreSQL)
    pub dollar_quotes: bool,
}

pub const GENERIC: Dialect = Dialect { backslash_escapes: false, hash_comments: false, dollar_quotes: false };
pub const MARIADB: Dialect = Dialect { backslash_escapes: true, hash_comments: true, dollar_quotes: false };
pub const POSTGRES: Dialect = Dialect { backslash_escapes: false, hash_comments: false, dollar_quotes: true };

fn starts(chars: &[char], i: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, p)| chars.get(i + k) == Some(&p))
}

/// Char ranges of each statement that contains something other than whitespace and comments.
/// A statement's range starts at its first non-whitespace char and includes its closing `;`.
pub fn split(text: &str, d: Dialect) -> Vec<Range<usize>> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = vec![];
    let mut stmt_start: Option<usize> = None; // first char of the current statement (incl. leading comments)
    let mut has_code = false; // saw something other than whitespace/comments
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if stmt_start.is_none() && !c.is_whitespace() {
            stmt_start = Some(i);
        }
        match c {
            '\'' | '"' | '`' => {
                has_code = true;
                i += 1;
                while i < n {
                    if d.backslash_escapes && c != '`' && chars[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if chars[i] == c {
                        if chars.get(i + 1) == Some(&c) {
                            i += 2; // doubled quote
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
            }
            '#' if d.hash_comments => {
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i < n && !starts(&chars, i, "*/") {
                    i += 1;
                }
                i += 2;
            }
            '$' if d.dollar_quotes => {
                // $tag$ opens a dollar-quoted string if tag is empty or an identifier
                let mut j = i + 1;
                while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                if chars.get(j) == Some(&'$') && !(j > i + 1 && chars[i + 1].is_ascii_digit()) {
                    has_code = true;
                    let tag: String = chars[i..=j].iter().collect();
                    i = j + 1;
                    while i < n && !starts(&chars, i, &tag) {
                        i += 1;
                    }
                    i = (i + tag.chars().count()).min(n);
                } else {
                    has_code = true;
                    i += 1;
                }
            }
            ';' => {
                i += 1;
                if has_code {
                    out.push(stmt_start.unwrap_or(i - 1)..i);
                }
                stmt_start = None;
                has_code = false;
            }
            c if c.is_whitespace() => i += 1,
            _ => {
                has_code = true;
                i += 1;
            }
        }
    }
    if has_code && let Some(s) = stmt_start {
        out.push(s..n);
    }
    // trim trailing whitespace from the last statement
    for r in out.iter_mut() {
        while r.end > r.start && chars[r.end - 1].is_whitespace() {
            r.end -= 1;
        }
    }
    out
}

/// The statement the cursor is in. Between statements the one just before the cursor wins
/// (like running the statement you just finished typing); before the first statement, the first.
pub fn statement_at(text: &str, cursor: usize, d: Dialect) -> Option<Range<usize>> {
    let stmts = split(text, d);
    if let Some(s) = stmts.iter().find(|s| cursor >= s.start && cursor <= s.end) {
        return Some(s.clone());
    }
    stmts.iter().rev().find(|s| s.end < cursor).or_else(|| stmts.first()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str, d: Dialect) -> Vec<String> {
        let chars: Vec<char> = src.chars().collect();
        split(src, d).into_iter().map(|r| chars[r].iter().collect()).collect()
    }

    fn at(src: &str, d: Dialect) -> Option<String> {
        let pos = src.chars().position(|c| c == '|').unwrap();
        let text: String = src.chars().filter(|c| *c != '|').collect();
        let chars: Vec<char> = text.chars().collect();
        statement_at(&text, pos, d).map(|r| chars[r].iter().collect())
    }

    #[test]
    fn splits_on_semicolons_and_trims() {
        assert_eq!(texts("select 1; select 2;\nselect 3", GENERIC), vec!["select 1;", "select 2;", "select 3"]);
        assert_eq!(texts("  select 1  ;  ", GENERIC), vec!["select 1  ;"]);
        assert!(texts("", GENERIC).is_empty());
        assert!(texts("  \n ; ;\n", GENERIC).is_empty(), "empty statements are skipped");
        assert!(texts("-- only a comment\n/* another */", GENERIC).is_empty());
    }

    #[test]
    fn semicolons_in_strings_identifiers_and_comments_do_not_split() {
        assert_eq!(texts("select 'a;b'; select 2", GENERIC), vec!["select 'a;b';", "select 2"]);
        assert_eq!(texts("select \"a;b\", `c;d`; select 2", GENERIC).len(), 2);
        assert_eq!(texts("select 1 -- a;b\n, 2; select 3", GENERIC), vec!["select 1 -- a;b\n, 2;", "select 3"]);
        assert_eq!(texts("select /* ; */ 1; select 2", GENERIC).len(), 2);
        assert_eq!(texts("select 'it''s;'; select 2", GENERIC).len(), 2, "doubled quote escapes");
    }

    #[test]
    fn leading_comments_belong_to_the_next_statement() {
        assert_eq!(texts("select 1;\n-- next\nselect 2;", GENERIC), vec!["select 1;", "-- next\nselect 2;"]);
    }

    #[test]
    fn backslash_escapes_only_in_mariadb() {
        let src = r"select 'a\'; select 2";
        assert_eq!(texts(src, MARIADB).len(), 1, "MariaDB: the quote is escaped, the string runs on");
        assert_eq!(texts(src, POSTGRES).len(), 2, "PostgreSQL: backslash is an ordinary char");
        assert_eq!(texts("select 1 # c;d\n; select 2", MARIADB).len(), 2);
        assert_eq!(texts("select 1 # c;d\n; select 2", GENERIC).len(), 3, "# is not a comment elsewhere");
    }

    #[test]
    fn dollar_quoting_in_postgres() {
        let src = "create function f() returns int as $$ select 1; select 2; $$ language sql; select 3";
        assert_eq!(texts(src, POSTGRES).len(), 2, "{:?}", texts(src, POSTGRES));
        let tagged = "do $body$ begin; end; $body$; select 1";
        assert_eq!(texts(tagged, POSTGRES).len(), 2);
        assert_eq!(texts("select $1; select $2", POSTGRES).len(), 2, "$1 is a parameter, not a quote");
        assert_eq!(texts("select $$a;b$$; select 2", GENERIC).len(), 3, "not special outside PostgreSQL");
    }

    #[test]
    fn unterminated_constructs_do_not_panic() {
        for src in ["select 'abc", "select \"abc", "select /* abc", "select $$abc", "select 'a\\", "$", "$$", "/*", "--"] {
            for d in [GENERIC, MARIADB, POSTGRES] {
                let _ = split(src, d);
            }
        }
    }

    #[test]
    fn statement_under_the_cursor() {
        assert_eq!(at("select 1;\nselec|t 2;\nselect 3;", GENERIC).as_deref(), Some("select 2;"));
        assert_eq!(at("|select 1;\nselect 2;", GENERIC).as_deref(), Some("select 1;"));
        assert_eq!(at("select 1;|\nselect 2;", GENERIC).as_deref(), Some("select 1;"), "right after the semicolon");
        assert_eq!(at("select 1;\nselect 2|", GENERIC).as_deref(), Some("select 2"));
        assert_eq!(at("select\n  a,\n  |b\nfrom t;", GENERIC).as_deref(), Some("select\n  a,\n  b\nfrom t;"), "multi-line statement");
    }

    #[test]
    fn between_statements_the_previous_one_is_chosen() {
        assert_eq!(at("select 1;\n\n|\n\nselect 2;", GENERIC).as_deref(), Some("select 1;"));
        assert_eq!(at("\n|\nselect 1;", GENERIC).as_deref(), Some("select 1;"), "before the first: the first");
        assert_eq!(at("select 1;\n   |", GENERIC).as_deref(), Some("select 1;"), "after the last");
        assert_eq!(at("|", GENERIC), None);
        assert_eq!(at("-- nothing|", GENERIC), None);
    }

    #[test]
    fn unicode_offsets_are_in_chars() {
        assert_eq!(at("select 'é€';\nselect '日|本';", GENERIC).as_deref(), Some("select '日本';"));
    }
}
