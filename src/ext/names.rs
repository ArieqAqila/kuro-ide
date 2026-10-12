//! Names read from the user's own files for an extension (`names_from`), by one of a fixed set of built-in readers.
//!
//! The paths come only from the user's config (`[ext.<name>] headers = ["/path/*.h"]`); an extension cannot name a path. Files are read,
//! never executed, and everything is capped.

use super::lang::Role;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const MAX_FILES: usize = 4000;
pub const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_NAMES: usize = 100_000;
const MAX_NAME_LEN: usize = 64;

/// Expand `dir/pattern` where only the file-name part may contain `*`, `?` or `[a-z]`.
pub fn expand(pattern: &str) -> Result<Vec<PathBuf>, String> {
    let path = Path::new(pattern);
    let (dir, name) = match (path.parent(), path.file_name().and_then(|n| n.to_str())) {
        (Some(d), Some(n)) => (if d.as_os_str().is_empty() { Path::new(".") } else { d }, n),
        _ => return Err(format!("`{pattern}` is not a file pattern")),
    };
    if dir.to_string_lossy().contains(['*', '?', '[']) {
        return Err(format!("`{pattern}`: wildcards are only allowed in the file name, not in folders"));
    }
    if !name.contains(['*', '?', '[']) {
        return Ok(if path.is_file() { vec![path.to_path_buf()] } else { vec![] });
    }
    let re = Regex::new(&super::glob_to_regex(name)).map_err(|e| format!("`{pattern}`: {e}"))?;
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && re.is_match(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    out.sort();
    Ok(out)
}

/// `TYPE name;` and `TYPE name[N];` members inside `typedef struct … { … }` (C headers).
pub fn c_struct_members(text: &str, out: &mut Vec<String>) {
    let member = Regex::new(r"^\s*(?:unsigned\s+|signed\s+|const\s+)*[A-Za-z_][A-Za-z0-9_]*\s+\**\s*([A-Za-z_][A-Za-z0-9_]*)\s*(?:\[[^\]]*\]\s*)*(?:,\s*\**\s*[A-Za-z_][A-Za-z0-9_]*\s*(?:\[[^\]]*\]\s*)*)*;").unwrap();
    let mut depth = 0usize;
    let mut in_struct = false;
    for line in text.lines() {
        let code = line.split("/*").next().unwrap_or(line);
        if !in_struct {
            if code.contains("struct") && code.contains('{') {
                in_struct = true;
                depth = 1;
            }
            continue;
        }
        depth += code.matches('{').count();
        let closes = code.matches('}').count();
        if depth <= closes {
            in_struct = false;
            continue;
        }
        depth -= closes;
        if depth == 1
            && let Some(c) = member.captures(code)
        {
            out.push(c[1].to_string());
        }
    }
}

/// `#define name -1601` (negative numbers: ksystem's index and table ids).
pub fn c_negative_defines(text: &str, out: &mut Vec<String>) {
    let define = Regex::new(r"^\s*#\s*define\s+([A-Za-z_][A-Za-z0-9_]*)\s+-\s*\d+").unwrap();
    out.extend(text.lines().filter_map(|l| define.captures(l).map(|c| c[1].to_string())));
}

pub fn parse_with(parser: &str, text: &str, out: &mut Vec<String>) {
    match parser {
        "c-struct-members" => c_struct_members(text, out),
        "c-negative-defines" => c_negative_defines(text, out),
        _ => {}
    }
}

/// Read every file matched by `patterns`, run `parser` on each, and map each name found to `role`.
/// Problems (a pattern that matches nothing, a file that cannot be read, a limit) are returned as warnings.
pub fn load(patterns: &[String], parser: &str, role: Role, into: &mut HashMap<String, Role>, warnings: &mut Vec<String>) {
    let mut files = 0usize;
    let mut bytes = 0u64;
    for pat in patterns {
        let paths = match expand(pat) {
            Ok(p) => p,
            Err(e) => {
                warnings.push(e);
                continue;
            }
        };
        if paths.is_empty() {
            warnings.push(format!("`{pat}` matches no files"));
        }
        for p in paths {
            files += 1;
            if files > MAX_FILES {
                warnings.push(format!("more than {MAX_FILES} files match; the rest are ignored"));
                return;
            }
            let Ok(meta) = std::fs::metadata(&p) else { continue };
            bytes += meta.len();
            if bytes > MAX_TOTAL_BYTES {
                warnings.push(format!("more than {} MiB of files match; the rest are ignored", MAX_TOTAL_BYTES / 1024 / 1024));
                return;
            }
            let Ok(raw) = std::fs::read(&p) else {
                warnings.push(format!("cannot read {}", p.display()));
                continue;
            };
            let text = String::from_utf8_lossy(&raw);
            let mut names = Vec::new();
            parse_with(parser, &text, &mut names);
            for n in names {
                if n.len() <= MAX_NAME_LEN {
                    into.entry(n).or_insert(role);
                }
                if into.len() >= MAX_NAMES {
                    warnings.push(format!("more than {MAX_NAMES} names; the rest are ignored"));
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_members_are_found_and_comments_and_other_lines_ignored() {
        let h = "#ifdef INDEX_INFORMATION\nxxidx  xxkey\n#endif\n#define xx 1\n\ntypedef struct xx { /* Demo */\nLONG           xxkey;                 /*0,Key*/\nTEXT           xxname[21];            /*4,Name*/\ndouble         xxamt;\nunsigned int   xxcount, xxother[3];\n} XX;\n\nint standalone;\n";
        let mut got = Vec::new();
        c_struct_members(h, &mut got);
        assert_eq!(got, vec!["xxkey", "xxname", "xxamt", "xxcount"], "{got:?}");
    }

    #[test]
    fn nested_braces_and_non_struct_files_are_handled() {
        let mut got = Vec::new();
        c_struct_members("typedef struct a {\nLONG one;\nstruct { LONG inner; } nested;\nLONG two;\n} A;\n", &mut got);
        assert!(got.contains(&"one".to_string()) && got.contains(&"two".to_string()), "{got:?}");
        let mut none = Vec::new();
        c_struct_members("just some log text;\nscr904: failed;\n", &mut none);
        assert!(none.is_empty());
    }

    #[test]
    fn negative_defines_are_the_index_names() {
        let mut got = Vec::new();
        c_negative_defines(
            "#define xxdat -1600\n#define xxkeyidx         -1601\n#define POSITIVE 5\n# define spaced -7\n#define x -\n",
            &mut got,
        );
        assert_eq!(got, vec!["xxdat", "xxkeyidx", "spaced"]);
    }

    #[test]
    fn only_the_file_name_may_have_wildcards() {
        assert!(expand("/some/*/x.h").unwrap_err().contains("only allowed in the file name"));
        assert!(expand("").is_err());
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.h"), "").unwrap();
        std::fs::write(d.path().join("b.h"), "").unwrap();
        std::fs::write(d.path().join("c.txt"), "").unwrap();
        std::fs::create_dir(d.path().join("d.h")).unwrap();
        let p = format!("{}/*.h", d.path().display());
        let got: Vec<String> = expand(&p).unwrap().iter().map(|x| x.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(got, vec!["a.h", "b.h"], "regular files only, sorted");
        assert_eq!(expand(&format!("{}/a.h", d.path().display())).unwrap().len(), 1);
        assert!(expand(&format!("{}/missing.h", d.path().display())).unwrap().is_empty());
    }
}
