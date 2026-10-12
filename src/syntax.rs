//! Table-driven syntax highlighting.
//!
//! One generic tokenizer is parameterised by a `LangSpec`. To add a language,
//! add a `LangSpec` constant and list it in `LANGS` (see docs/CUSTOMIZATION.md).

use std::path::Path;

pub use crate::langs::{C, CPP, CSS, HTML, JAVA, JAVASCRIPT, PHP, PHP_CODE, RUBY, RUST, TOML, TYPESCRIPT, XML, YAML};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Keyword,
    Builtin,
    Literal,
    String,
    Number,
    Comment,
    /// JSON object keys and similar names.
    Key,
    Operator,
    /// A colour role defined by the current language's extension: index into its `ExtData::roles`.
    Ext(u8),
    /// A bracket; the payload is its nesting depth (0 = outermost).
    Bracket(u8),
    /// An unmatched or mismatched bracket.
    BracketError,
}

#[derive(Debug)]
pub struct LangSpec {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub filenames: &'static [&'static str],
    /// Substrings of a `#!` first line that select this language.
    pub shebangs: &'static [&'static str],
    pub keywords: &'static [&'static str],
    pub builtins: &'static [&'static str],
    pub literals: &'static [&'static str],
    pub line_comments: &'static [&'static str],
    pub block_comment: Option<(&'static str, &'static str)>,
    pub string_quotes: &'static [char],
    /// Python-style `"""` / `'''` strings that span lines.
    pub triple_quotes: bool,
    /// Go-style raw string quote that spans lines.
    pub raw_quote: Option<char>,
    pub case_insensitive: bool,
    /// Shell: `$name`, `${name}` highlighted as builtins; `#` only starts a comment at a word start.
    pub shell_vars: bool,
    /// Strings followed by `:` are keys (JSON).
    pub key_colon: bool,
    /// Operator tokens; the longest match at a position wins.
    pub operators: &'static [&'static str],
    /// Colour `()[]{}` by nesting depth.
    pub brackets: bool,
    /// Flag unmatched / mismatched brackets (off for shell, where `case x in a)` is legal).
    pub bracket_errors: bool,
    /// `/` can start a regular-expression literal (JavaScript, TypeScript).
    pub regex_literals: bool,
    /// Characters that start a variable-like name drawn as a builtin: `$name` (PHP), `@name` (Ruby, Java annotations).
    pub sigils: &'static [char],
    /// A bare word at the start of a line, followed by this character, is a key (YAML `:`, TOML `=`).
    pub bare_key_sep: Option<char>,
    /// Lines like `[section]` are headers (TOML).
    pub section_headers: bool,
    /// Rust: `'a` is a lifetime (a builtin) and `'x'` a character literal (a string).
    pub lifetimes: bool,
    /// C family: `#include`, `#define`, ... at the start of a line.
    pub preprocessor: bool,
    /// Markup languages (HTML, XML, PHP, CSS) use a small state machine that hands regions to other languages.
    pub markup: Option<Markup>,
    /// A string that is not closed on its line continues on the next one (only for the quotes in `string_quotes`).
    pub multiline_strings: bool,
    /// Extra names `:setlang` accepts (extension languages; the built-in aliases live in `by_name`).
    pub aliases: &'static [&'static str],
    /// Rules, declared names and colour roles that come from an extension (see `src/ext/lang.rs`).
    pub ext: Option<&'static crate::ext::lang::ExtData>,
}

/// Which markup tokenizer a language uses (see `src/markup.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Markup {
    Html,
    Xml,
    /// HTML with `<?php ... ?>` regions.
    Php,
    Css,
}

pub(crate) const EMPTY: &[&str] = &[];

pub(crate) const BASE: LangSpec = LangSpec {
    name: "Plain Text",
    extensions: EMPTY,
    filenames: EMPTY,
    shebangs: EMPTY,
    keywords: EMPTY,
    builtins: EMPTY,
    literals: EMPTY,
    line_comments: EMPTY,
    block_comment: None,
    string_quotes: &[],
    triple_quotes: false,
    raw_quote: None,
    case_insensitive: false,
    shell_vars: false,
    key_colon: false,
    operators: EMPTY,
    brackets: false,
    bracket_errors: false,
    regex_literals: false,
    sigils: &[],
    bare_key_sep: None,
    section_headers: false,
    lifetimes: false,
    preprocessor: false,
    markup: None,
    multiline_strings: false,
    aliases: &[],
    ext: None,
};

pub static PLAIN: LangSpec = BASE;

pub static SQL: LangSpec = LangSpec {
    name: "SQL",
    extensions: &["sql"],
    keywords: &[
        "select",
        "from",
        "where",
        "and",
        "or",
        "not",
        "insert",
        "into",
        "values",
        "update",
        "set",
        "delete",
        "create",
        "alter",
        "drop",
        "table",
        "index",
        "view",
        "database",
        "schema",
        "join",
        "inner",
        "left",
        "right",
        "full",
        "outer",
        "cross",
        "on",
        "using",
        "group",
        "by",
        "order",
        "having",
        "limit",
        "offset",
        "union",
        "all",
        "distinct",
        "as",
        "in",
        "is",
        "like",
        "ilike",
        "between",
        "exists",
        "case",
        "when",
        "then",
        "else",
        "end",
        "with",
        "recursive",
        "primary",
        "key",
        "foreign",
        "references",
        "default",
        "constraint",
        "unique",
        "check",
        "returning",
        "begin",
        "commit",
        "rollback",
        "transaction",
        "grant",
        "revoke",
        "truncate",
        "explain",
        "analyze",
        "desc",
        "asc",
        "if",
        "cascade",
        "temporary",
        "temp",
        "over",
        "partition",
        "window",
        "fetch",
        "next",
        "only",
        "rows",
        "row",
    ],
    builtins: &[
        "count",
        "sum",
        "avg",
        "min",
        "max",
        "coalesce",
        "nullif",
        "now",
        "length",
        "lower",
        "upper",
        "substr",
        "substring",
        "concat",
        "cast",
        "round",
        "floor",
        "ceil",
        "abs",
        "int",
        "integer",
        "bigint",
        "smallint",
        "serial",
        "text",
        "varchar",
        "char",
        "boolean",
        "bool",
        "date",
        "time",
        "timestamp",
        "timestamptz",
        "numeric",
        "decimal",
        "float",
        "double",
        "json",
        "jsonb",
        "uuid",
        "blob",
        "generate_series",
        "sleep",
        "pg_sleep",
        "row_number",
        "rank",
    ],
    literals: &["null", "true", "false"],
    line_comments: &["--"],
    block_comment: Some(("/*", "*/")),
    string_quotes: &['\'', '"', '`'],
    case_insensitive: true,
    operators: &[
        "->>", "->", "#>>", "#>", "::", "<>", "!=", "<=", ">=", "||", "&&", "<<", ">>", "=", "<", ">", "+", "-", "*", "/", "%", "|", "&",
        "^", "~",
    ],
    brackets: true,
    bracket_errors: true,
    ..BASE
};

pub static GO: LangSpec = LangSpec {
    name: "Go",
    extensions: &["go"],
    filenames: &["go.mod"],
    keywords: &[
        "break",
        "case",
        "chan",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "fallthrough",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "interface",
        "map",
        "package",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "type",
        "var",
    ],
    builtins: &[
        "append",
        "cap",
        "clear",
        "close",
        "complex",
        "copy",
        "delete",
        "imag",
        "len",
        "make",
        "max",
        "min",
        "new",
        "panic",
        "print",
        "println",
        "real",
        "recover",
        "any",
        "bool",
        "byte",
        "comparable",
        "error",
        "float32",
        "float64",
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "rune",
        "string",
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "uintptr",
    ],
    literals: &["true", "false", "nil", "iota"],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    string_quotes: &['"', '\''],
    raw_quote: Some('`'),
    operators: &[
        "<<=", ">>=", "&^=", "...", "&&", "||", "<-", "++", "--", "==", "!=", "<=", ">=", ":=", "+=", "-=", "*=", "/=", "%=", "&=", "|=",
        "^=", "<<", ">>", "&^", "+", "-", "*", "/", "%", "=", "<", ">", "!", "&", "|", "^",
    ],
    brackets: true,
    bracket_errors: true,
    ..BASE
};

pub static PYTHON: LangSpec = LangSpec {
    name: "Python",
    extensions: &["py", "pyi", "pyw"],
    shebangs: &["python"],
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for",
        "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with",
        "yield", "match", "case",
    ],
    builtins: &[
        "print",
        "len",
        "range",
        "str",
        "int",
        "float",
        "bool",
        "list",
        "dict",
        "set",
        "tuple",
        "type",
        "isinstance",
        "open",
        "super",
        "self",
        "cls",
        "enumerate",
        "zip",
        "map",
        "filter",
        "sorted",
        "sum",
        "min",
        "max",
        "abs",
        "any",
        "all",
        "repr",
        "input",
    ],
    literals: &["True", "False", "None"],
    line_comments: &["#"],
    string_quotes: &['"', '\''],
    triple_quotes: true,
    operators: &[
        "**=", "//=", ">>=", "<<=", "->", "==", "!=", "<=", ">=", ":=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "@=", "**", "//",
        "<<", ">>", "+", "-", "*", "/", "%", "=", "<", ">", "&", "|", "^", "~", "@",
    ],
    brackets: true,
    bracket_errors: true,
    ..BASE
};

pub static SHELL: LangSpec = LangSpec {
    name: "Shell",
    extensions: &["sh", "bash", "zsh", "fish", "ksh"],
    filenames: &[".bashrc", ".zshrc", ".profile", ".bash_profile", "config.fish"],
    shebangs: &["sh", "bash", "zsh", "fish", "ksh"],
    keywords: &[
        "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function", "select", "time",
        "return", "break", "continue", "end", "switch", "begin", "and", "or", "not",
    ],
    builtins: &[
        "echo", "cd", "export", "set", "unset", "local", "read", "exit", "source", "alias", "test", "eval", "exec", "shift", "trap",
        "printf", "pwd", "type", "command", "true", "false", "declare", "readonly",
    ],
    literals: EMPTY,
    line_comments: &["#"],
    string_quotes: &['"', '\''],
    shell_vars: true,
    operators: &["&&", "||", ">>", "<<", ">&", "<&", "|&", ";;", "!=", "==", "=~", "=", ">", "<", "|", "&"],
    brackets: true,
    bracket_errors: false,
    ..BASE
};

pub static JSON: LangSpec = LangSpec {
    name: "JSON",
    extensions: &["json", "jsonc", "geojson"],
    filenames: &[".eslintrc", ".prettierrc"],
    literals: &["true", "false", "null"],
    string_quotes: &['"'],
    key_colon: true,
    brackets: true,
    bracket_errors: true,
    ..BASE
};

pub static LUA: LangSpec = LangSpec {
    name: "Lua",
    extensions: &["lua", "rockspec"],
    filenames: &[".luacheckrc"],
    shebangs: &["lua", "luajit"],
    keywords: &[
        "and", "break", "do", "else", "elseif", "end", "for", "function", "goto", "if", "in", "local", "not", "or", "repeat", "return",
        "then", "until", "while",
    ],
    builtins: &[
        "print",
        "pairs",
        "ipairs",
        "type",
        "tostring",
        "tonumber",
        "require",
        "pcall",
        "xpcall",
        "error",
        "assert",
        "select",
        "next",
        "setmetatable",
        "getmetatable",
        "rawget",
        "rawset",
        "rawequal",
        "rawlen",
        "unpack",
        "load",
        "dofile",
        "loadfile",
        "string",
        "table",
        "math",
        "io",
        "os",
        "coroutine",
        "debug",
        "utf8",
        "self",
        "_G",
        "_ENV",
    ],
    literals: &["nil", "true", "false"],
    line_comments: &["--"],
    block_comment: Some(("--[[", "]]")),
    string_quotes: &['"', '\''],
    operators: &[
        "...", "==", "~=", "<=", ">=", "..", "//", "<<", ">>", "::", "+", "-", "*", "/", "%", "^", "#", "&", "~", "|", "<", ">", "=",
    ],
    brackets: true,
    bracket_errors: true,
    ..BASE
};

/// Every selectable language, in the order the picker lists them.
pub static LANGS: &[&LangSpec] = &[
    &SQL,
    &GO,
    &PYTHON,
    &SHELL,
    &JSON,
    &LUA,
    &RUST,
    &C,
    &CPP,
    &JAVA,
    &JAVASCRIPT,
    &TYPESCRIPT,
    &HTML,
    &CSS,
    &PHP,
    &RUBY,
    &YAML,
    &TOML,
    &XML,
];

/// Every selectable mode: Plain Text first, then the languages.
pub fn all_modes() -> Vec<&'static LangSpec> {
    std::iter::once(&PLAIN).chain(LANGS.iter().copied()).collect()
}

/// Look a mode up by name or common alias, ignoring case (`:setlang`).
pub fn by_name(name: &str) -> Option<&'static LangSpec> {
    by_name_in(&[], name)
}

/// Like [`by_name`], also knowing the languages of installed extensions.
pub fn by_name_in(ext: &[&'static LangSpec], name: &str) -> Option<&'static LangSpec> {
    let n = name.trim().to_ascii_lowercase();
    if let Some(l) = ext.iter().find(|l| l.name.to_ascii_lowercase() == n || l.aliases.iter().any(|a| a.to_ascii_lowercase() == n)) {
        return Some(l);
    }
    let alias = match n.as_str() {
        "text" | "plain" | "plaintext" | "txt" | "none" => "plain text",
        "sh" | "bash" | "zsh" | "fish" | "ksh" => "shell",
        "golang" => "go",
        "py" | "python3" => "python",
        "luajit" => "lua",
        "rs" => "rust",
        "c++" | "cpp" | "cxx" | "cc" => "c++",
        "js" | "node" | "nodejs" | "ecmascript" => "javascript",
        "ts" => "typescript",
        "htm" | "xhtml" => "html",
        "rb" => "ruby",
        "yml" => "yaml",
        "phtml" => "php",
        "svg" => "xml",
        "postgres" | "postgresql" | "mysql" | "mariadb" => "sql",
        other => other,
    };
    all_modes().into_iter().find(|l| l.name.to_ascii_lowercase() == alias)
}

pub fn detect(path: Option<&Path>, first_line: &str) -> &'static LangSpec {
    detect_in(&[], path, first_line)
}

/// Pick the language for a file. Order: file names (extension rules, then built-in), file types and `#!` lines of extension
/// languages, the built-in file types and `#!` lines, and last a content marker on the first line (extension languages), so a marker
/// can never take over a file that has a known file type.
pub fn detect_in(ext: &[&'static LangSpec], path: Option<&Path>, first_line: &str) -> &'static LangSpec {
    if !ext.is_empty() {
        let name = path.and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("");
        let file_ext = path.and_then(|p| p.extension()).and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let data = |l: &&&'static LangSpec| l.ext;
        if !name.is_empty() {
            if let Some(l) = ext
                .iter()
                .find(|l| l.filenames.contains(&name) || data(l).is_some_and(|d| d.filename_globs.iter().any(|g| g.is_match(name))))
            {
                return l;
            }
            if let Some(l) = LANGS.iter().find(|l| l.filenames.contains(&name)) {
                return l;
            }
        }
        if let Some(l) =
            ext.iter().find(|l| (!file_ext.is_empty() && l.extensions.contains(&file_ext.as_str())) || shebang_matches(l, first_line))
        {
            return l;
        }
    }
    if let Some(p) = path {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        for l in LANGS {
            if l.filenames.contains(&name) || (!ext.is_empty() && l.extensions.contains(&ext.as_str())) {
                return l;
            }
        }
    }
    for l in LANGS {
        if shebang_matches(l, first_line) {
            return l;
        }
    }
    // last: a content marker on the first line, for files no file type claims (extension-less files, new buffers)
    if let Some(l) = ext.iter().find(|l| l.ext.and_then(|d| d.first_line.as_ref()).is_some_and(|r| r.is_match(first_line))) {
        return l;
    }
    &PLAIN
}

/// Does a `#!` first line name an interpreter of `l`? (the basename, so `/usr/bin/env python3` matches `python`)
fn shebang_matches(l: &LangSpec, first_line: &str) -> bool {
    let Some(rest) = first_line.strip_prefix("#!") else { return false };
    let words: Vec<&str> = rest.split_whitespace().collect();
    let interp = words.iter().rev().find(|w| !w.starts_with('-')).copied().unwrap_or("");
    let base = interp.rsplit('/').next().unwrap_or("");
    l.shebangs.iter().any(|s| base == *s || (base.starts_with(s) && base[s.len()..].chars().all(|c| c.is_ascii_digit() || c == '.')))
}

/// Every selectable mode including the languages of extensions (listed last).
pub fn all_modes_in(ext: &[&'static LangSpec]) -> Vec<&'static LangSpec> {
    all_modes().into_iter().chain(ext.iter().copied()).collect()
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
enum State {
    #[default]
    Normal,
    Block,
    /// Inside a multi-line string closed by `quote` (repeated 3x when `triple`).
    Multi {
        quote: char,
        triple: bool,
    },
}

pub(crate) fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}
pub(crate) fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub(crate) fn starts_with_at(chars: &[char], i: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, p)| chars.get(i + k) == Some(&p))
}

/// Can a `/` at `i` start a regex literal? Yes when the previous token cannot end an operand.
fn regex_allowed(chars: &[char], i: usize) -> bool {
    let mut j = i;
    while j > 0 && chars[j - 1].is_whitespace() {
        j -= 1;
    }
    let Some(&prev) = j.checked_sub(1).and_then(|k| chars.get(k)) else { return true };
    if is_ident(prev) {
        // after a word: only keywords that are followed by an expression
        let end = j;
        let mut start = j;
        while start > 0 && is_ident(chars[start - 1]) {
            start -= 1;
        }
        let w: String = chars[start..end].iter().collect();
        return matches!(
            w.as_str(),
            "return" | "typeof" | "case" | "in" | "of" | "delete" | "void" | "throw" | "new" | "else" | "do" | "yield" | "await"
        );
    }
    !matches!(prev, ')' | ']' | '"' | '\'' | '`')
}

fn lookup(spec: &LangSpec, word: &str) -> Kind {
    let w;
    let word = if spec.case_insensitive {
        w = word.to_lowercase();
        w.as_str()
    } else {
        word
    };
    if spec.keywords.contains(&word) {
        Kind::Keyword
    } else if spec.literals.contains(&word) {
        Kind::Literal
    } else if spec.builtins.contains(&word) {
        Kind::Builtin
    } else {
        Kind::Plain
    }
}

/// Highlight every line; result[i][j] is the kind of char j of line i.
pub fn highlight(spec: &LangSpec, lines: &[String]) -> Vec<Vec<Kind>> {
    if spec.name == PLAIN.name {
        return lines.iter().map(|l| vec![Kind::Plain; l.chars().count()]).collect();
    }
    if let Some(m) = spec.markup {
        return crate::markup::highlight(spec, m, lines);
    }
    let mut state = State::Normal;
    // open brackets: (char, line, column)
    let mut stack: Vec<(char, usize, usize)> = Vec::new();
    let mut out = Vec::with_capacity(lines.len());
    for (li, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut kinds = vec![Kind::Plain; chars.len()];
        state = highlight_line(spec, &chars, &mut kinds, state, li, &mut stack);
        out.push(kinds);
    }
    if spec.bracket_errors {
        for (_, l, c) in stack {
            out[l][c] = Kind::BracketError;
        }
    }
    if let Some(e) = spec.ext {
        apply_ext(e, lines, &mut out);
    }
    out
}

/// Extension rules on top of the generic pass: regex rules (in their section), then the names declared in the file and the names read
/// from the user's own files. None of them overrides a comment, a string or a bracket.
fn apply_ext(e: &crate::ext::lang::ExtData, lines: &[String], out: &mut [Vec<Kind>]) {
    use std::collections::HashMap;
    let char_index = |line: &str, byte: usize| line[..byte].chars().count();
    let paint = |kinds: &mut [Kind], a: usize, b: usize, k: Kind| {
        for x in kinds.iter_mut().take(b).skip(a) {
            if !matches!(*x, Kind::Comment | Kind::String | Kind::Bracket(_) | Kind::BracketError) {
                *x = k;
            }
        }
    };
    // declared names (per file)
    let mut declared: HashMap<String, crate::ext::lang::Role> = HashMap::new();
    for d in &e.declares {
        let mut in_decl = false;
        for (i, l) in lines.iter().enumerate() {
            if e.section_header.is_match(l) {
                in_decl = d.section.is_match(l);
                continue;
            }
            if in_decl
                && !matches!(out[i].first(), Some(Kind::Comment))
                && let Some(c) = d.name.captures(l)
                && let Some(m) = c.get(1)
            {
                declared.entry(m.as_str().to_string()).or_insert(d.role);
            }
        }
    }
    // regex rules
    for r in &e.rules {
        let mut active = r.in_section.is_none();
        for (i, l) in lines.iter().enumerate() {
            if let Some(start) = &r.in_section {
                if start.is_match(l) {
                    active = true;
                } else if r.end_section.as_ref().map_or(e.section_header.is_match(l), |end| end.is_match(l)) {
                    active = false;
                }
                if !active {
                    continue;
                }
            }
            for m in r.pattern.find_iter(l) {
                if m.is_empty() {
                    continue;
                }
                let (a, b) = (char_index(l, m.start()), char_index(l, m.end()));
                paint(&mut out[i], a, b, r.role.kind());
            }
        }
    }
    // names: identifiers that are still plain
    let external = e.external.read().unwrap();
    if declared.is_empty() && external.is_empty() {
        return;
    }
    for (i, l) in lines.iter().enumerate() {
        let chars: Vec<char> = l.chars().collect();
        let mut j = 0;
        while j < chars.len() {
            if is_ident_start(chars[j]) && (j == 0 || !is_ident(chars[j - 1])) {
                let start = j;
                while j < chars.len() && is_ident(chars[j]) {
                    j += 1;
                }
                if out[i][start] == Kind::Plain {
                    let word: String = chars[start..j].iter().collect();
                    if let Some(role) = declared.get(&word).or_else(|| external.get(&word)) {
                        let k = role.kind();
                        for x in &mut out[i][start..j] {
                            *x = k;
                        }
                    }
                }
            } else {
                j += 1;
            }
        }
    }
}

pub(crate) fn fill(kinds: &mut [Kind], from: usize, to: usize, k: Kind) {
    let to = to.min(kinds.len());
    for x in kinds.iter_mut().take(to).skip(from) {
        *x = k;
    }
}

fn highlight_line(
    spec: &LangSpec,
    chars: &[char],
    kinds: &mut [Kind],
    mut state: State,
    line: usize,
    stack: &mut Vec<(char, usize, usize)>,
) -> State {
    let n = chars.len();
    let mut i = 0;
    while i < n {
        match state {
            State::Block => {
                let end = spec.block_comment.map(|b| b.1).unwrap_or("");
                let start = i;
                while i < n && !starts_with_at(chars, i, end) {
                    i += 1;
                }
                if i < n {
                    i += end.chars().count();
                    state = State::Normal;
                }
                fill(kinds, start, i, Kind::Comment);
                continue;
            }
            State::Multi { quote, triple } => {
                let start = i;
                let closer: String = if triple { std::iter::repeat_n(quote, 3).collect() } else { quote.to_string() };
                let mut closed = false;
                while i < n {
                    if chars[i] == '\\' && spec.raw_quote != Some(quote) {
                        i += 2;
                        continue;
                    }
                    if starts_with_at(chars, i, &closer) {
                        i += closer.chars().count();
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                fill(kinds, start, i.min(n), Kind::String);
                if closed {
                    state = State::Normal;
                }
                continue;
            }
            State::Normal => {}
        }

        let c = chars[i];
        let first_on_line = chars[..i].iter().all(|ch| ch.is_whitespace());

        // C family preprocessor directives: `#include`, `#define`, ...
        if spec.preprocessor && c == '#' && first_on_line {
            let mut j = i + 1;
            while j < n && chars[j].is_whitespace() {
                j += 1;
            }
            while j < n && is_ident(chars[j]) {
                j += 1;
            }
            fill(kinds, i, j, Kind::Keyword);
            i = j;
            continue;
        }
        // `[section]` headers
        if spec.section_headers && c == '[' && first_on_line {
            let end = chars.iter().rposition(|x| *x == ']').map(|e| e + 1).unwrap_or(n);
            if end > i {
                fill(kinds, i, end, Kind::Keyword);
                i = end;
                continue;
            }
        }
        // bare `key:` / `key =` at the start of a line (YAML, TOML); a leading `- ` of a YAML list is skipped
        if let Some(sep) = spec.bare_key_sep
            && (is_ident_start(c) || c.is_ascii_digit())
            && chars[..i].iter().all(|ch| ch.is_whitespace() || *ch == '-')
        {
            let mut j = i;
            while j < n && (is_ident(chars[j]) || chars[j] == '-' || chars[j] == '.') {
                j += 1;
            }
            let mut k = j;
            while k < n && chars[k] == ' ' {
                k += 1;
            }
            let after = chars.get(k + 1);
            if chars.get(k) == Some(&sep) && (sep != ':' || after.is_none() || after == Some(&' ')) {
                fill(kinds, i, j, Kind::Key);
                i = j;
                continue;
            }
        }
        // Rust lifetimes and character literals
        if spec.lifetimes && c == '\'' {
            let close = if chars.get(i + 1) == Some(&'\\') {
                (i + 2..n.min(i + 12)).find(|&j| chars[j] == '\'')
            } else if chars.get(i + 2) == Some(&'\'') {
                Some(i + 2)
            } else {
                None
            };
            if let Some(e) = close {
                fill(kinds, i, e + 1, Kind::String);
                i = e + 1;
            } else {
                let mut j = i + 1;
                while j < n && is_ident(chars[j]) {
                    j += 1;
                }
                fill(kinds, i, j, Kind::Builtin);
                i = j.max(i + 1);
            }
            continue;
        }
        // `$name`, `@name`
        if spec.sigils.contains(&c) && chars.get(i + 1).is_some_and(|d| is_ident_start(*d) || *d == c) {
            let mut j = i + 1;
            while j < n && (is_ident(chars[j]) || chars[j] == c) {
                j += 1;
            }
            fill(kinds, i, j, Kind::Builtin);
            i = j;
            continue;
        }

        // comments
        // a block opener that begins with the line-comment marker (Lua's `--[[`) is a block comment
        let block_here = spec.block_comment.is_some_and(|(open, _)| starts_with_at(chars, i, open));
        if let Some(lc) = spec.line_comments.iter().find(|lc| !block_here && starts_with_at(chars, i, lc)) {
            let word_start = i == 0 || chars[i - 1].is_whitespace() || chars[i - 1] == ';';
            if !(spec.shell_vars && *lc == "#" && !word_start) {
                fill(kinds, i, n, Kind::Comment);
                return state;
            }
        }
        if let Some((open, close)) = spec.block_comment
            && starts_with_at(chars, i, open)
        {
            let start = i;
            i += open.chars().count();
            while i < n && !starts_with_at(chars, i, close) {
                i += 1;
            }
            if i < n {
                i += close.chars().count();
            } else {
                state = State::Block;
            }
            fill(kinds, start, i.min(n), Kind::Comment);
            continue;
        }

        // strings
        if spec.raw_quote == Some(c) {
            let start = i;
            i += 1;
            while i < n && chars[i] != c {
                i += 1;
            }
            if i < n {
                i += 1;
            } else {
                state = State::Multi { quote: c, triple: false };
            }
            fill(kinds, start, i.min(n), Kind::String);
            continue;
        }
        if spec.string_quotes.contains(&c) {
            let start = i;
            if spec.triple_quotes && starts_with_at(chars, i, &std::iter::repeat_n(c, 3).collect::<String>()) {
                state = State::Multi { quote: c, triple: true };
                i += 3;
                fill(kinds, start, i, Kind::String);
                continue;
            }
            i += 1;
            while i < n && chars[i] != c {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            if i >= n && spec.multiline_strings {
                state = State::Multi { quote: c, triple: false };
            }
            i = (i + 1).min(n);
            let mut kind = Kind::String;
            if spec.key_colon {
                let mut j = i;
                while j < n && chars[j].is_whitespace() {
                    j += 1;
                }
                if chars.get(j) == Some(&':') {
                    kind = Kind::Key;
                }
            }
            fill(kinds, start, i, kind);
            continue;
        }

        // regular-expression literal: `/` where an operand is expected
        if spec.regex_literals && c == '/' && regex_allowed(chars, i) {
            let mut j = i + 1;
            let mut in_class = false;
            let mut closed = false;
            while j < n {
                match chars[j] {
                    '\\' => j += 1,
                    '[' => in_class = true,
                    ']' => in_class = false,
                    '/' if !in_class => {
                        closed = true;
                        break;
                    }
                    _ => {}
                }
                j += 1;
            }
            if closed {
                j += 1;
                while j < n && chars[j].is_ascii_alphabetic() {
                    j += 1; // flags
                }
                fill(kinds, i, j.min(n), Kind::String);
                i = j.min(n);
                continue;
            }
        }

        // shell variables
        if spec.shell_vars && c == '$' {
            let start = i;
            i += 1;
            if chars.get(i) == Some(&'{') {
                while i < n && chars[i] != '}' {
                    i += 1;
                }
                i = (i + 1).min(n);
            } else {
                while i < n && (is_ident(chars[i]) || (i == start + 1 && "?!#@*$0123456789".contains(chars[i]))) {
                    i += 1;
                }
            }
            fill(kinds, start, i, Kind::Builtin);
            continue;
        }

        // numbers
        let prev_ident = i > 0 && is_ident(chars[i - 1]);
        if c.is_ascii_digit() && !prev_ident
            || (c == '-' && spec.key_colon && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) && !prev_ident)
        {
            let start = i;
            i += 1;
            while i < n
                && (chars[i].is_ascii_alphanumeric()
                    || (chars[i] == '.' && chars.get(i + 1) != Some(&'.'))
                    || chars[i] == '_'
                    || ((chars[i] == '+' || chars[i] == '-') && matches!(chars[i - 1], 'e' | 'E')))
            {
                i += 1;
            }
            fill(kinds, start, i, Kind::Number);
            continue;
        }

        // identifiers / keywords
        if is_ident_start(c) {
            let start = i;
            while i < n && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            fill(kinds, start, i, lookup(spec, &word));
            continue;
        }

        // brackets, coloured by nesting depth
        if spec.brackets && matches!(c, '(' | '[' | '{') {
            kinds[i] = Kind::Bracket(stack.len().min(255) as u8);
            stack.push((c, line, i));
            i += 1;
            continue;
        }
        if spec.brackets && matches!(c, ')' | ']' | '}') {
            let want = match c {
                ')' => '(',
                ']' => '[',
                _ => '{',
            };
            if stack.last().is_some_and(|(o, ..)| *o == want) {
                stack.pop();
                kinds[i] = Kind::Bracket(stack.len().min(255) as u8);
            } else if spec.bracket_errors {
                kinds[i] = Kind::BracketError;
            }
            i += 1;
            continue;
        }

        // operators: longest match
        if let Some(len) = spec.operators.iter().filter(|op| starts_with_at(chars, i, op)).map(|op| op.chars().count()).max() {
            fill(kinds, i, i + len, Kind::Operator);
            i += len;
            continue;
        }

        i += 1;
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hl(spec: &LangSpec, src: &str) -> Vec<Vec<Kind>> {
        let lines: Vec<String> = src.split('\n').map(String::from).collect();
        highlight(spec, &lines)
    }

    /// Kind of the first char of `needle` in line `l` of `src`.
    fn kind_of(spec: &LangSpec, src: &str, l: usize, needle: &str) -> Kind {
        let line = src.split('\n').nth(l).unwrap();
        let idx = line[..line.find(needle).unwrap()].chars().count();
        hl(spec, src)[l][idx]
    }

    #[test]
    fn detect_by_extension_and_name() {
        assert_eq!(detect(Some(Path::new("a/b/q.sql")), "").name, "SQL");
        assert_eq!(detect(Some(Path::new("main.go")), "").name, "Go");
        assert_eq!(detect(Some(Path::new("x.PY")), "").name, "Python");
        assert_eq!(detect(Some(Path::new("a.json")), "").name, "JSON");
        assert_eq!(detect(Some(Path::new(".bashrc")), "").name, "Shell");
        assert_eq!(detect(Some(Path::new("README")), "").name, "Plain Text");
        assert_eq!(detect(None, "").name, "Plain Text");
    }

    #[test]
    fn detect_by_shebang() {
        assert_eq!(detect(None, "#!/usr/bin/env python3").name, "Python");
        assert_eq!(detect(None, "#!/bin/bash").name, "Shell");
        assert_eq!(detect(Some(Path::new("script")), "#!/usr/bin/fish").name, "Shell");
        assert_eq!(detect(None, "#!/usr/bin/perl").name, "Plain Text");
    }

    #[test]
    fn by_name_finds_modes_and_aliases_ignoring_case() {
        assert_eq!(by_name("go").unwrap().name, "Go");
        assert_eq!(by_name("GOLANG").unwrap().name, "Go");
        assert_eq!(by_name("Python").unwrap().name, "Python");
        assert_eq!(by_name("py").unwrap().name, "Python");
        assert_eq!(by_name("fish").unwrap().name, "Shell");
        assert_eq!(by_name("sql").unwrap().name, "SQL");
        assert_eq!(by_name("json").unwrap().name, "JSON");
        assert_eq!(by_name("plain").unwrap().name, "Plain Text");
        assert_eq!(by_name("Plain Text").unwrap().name, "Plain Text");
        assert!(by_name("klingon").is_none());
        assert!(by_name("").is_none());
        assert_eq!(all_modes().len(), LANGS.len() + 1);
    }

    #[test]
    fn every_language_is_registered_and_named_uniquely() {
        let mut names: Vec<_> = LANGS.iter().map(|l| l.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(
            names,
            vec![
                "C",
                "C++",
                "CSS",
                "Go",
                "HTML",
                "JSON",
                "Java",
                "JavaScript",
                "Lua",
                "PHP",
                "Python",
                "Ruby",
                "Rust",
                "SQL",
                "Shell",
                "TOML",
                "TypeScript",
                "XML",
                "YAML"
            ]
        );
    }

    #[test]
    fn sql_keywords_case_insensitive_strings_comments() {
        let src = "SELECT name, 42 FROM t -- hi\nWHERE x = 'a''b' /* c */ AND y";
        assert_eq!(kind_of(&SQL, src, 0, "SELECT"), Kind::Keyword);
        assert_eq!(kind_of(&SQL, src, 0, "name"), Kind::Plain);
        assert_eq!(kind_of(&SQL, src, 0, "42"), Kind::Number);
        assert_eq!(kind_of(&SQL, src, 0, "-- hi"), Kind::Comment);
        assert_eq!(kind_of(&SQL, src, 1, "'a"), Kind::String);
        assert_eq!(kind_of(&SQL, src, 1, "/*"), Kind::Comment);
        assert_eq!(kind_of(&SQL, src, 1, "AND"), Kind::Keyword);
    }

    #[test]
    fn sql_block_comment_spans_lines() {
        let src = "a /* x\nselect\n*/ select";
        assert_eq!(kind_of(&SQL, src, 1, "select"), Kind::Comment);
        assert_eq!(kind_of(&SQL, src, 2, "select"), Kind::Keyword);
    }

    #[test]
    fn go_raw_string_spans_lines() {
        let src = "x := `a\nfunc\n` + func";
        assert_eq!(kind_of(Go_(), src, 1, "func"), Kind::String);
        assert_eq!(kind_of(Go_(), src, 2, "func"), Kind::Keyword);
    }

    #[allow(non_snake_case)]
    fn Go_() -> &'static LangSpec {
        &GO
    }

    #[test]
    fn go_basics() {
        let src = "func main() { fmt.Println(\"hi\", 1.5, nil) } // end";
        assert_eq!(kind_of(&GO, src, 0, "func"), Kind::Keyword);
        assert_eq!(kind_of(&GO, src, 0, "\"hi\""), Kind::String);
        assert_eq!(kind_of(&GO, src, 0, "1.5"), Kind::Number);
        assert_eq!(kind_of(&GO, src, 0, "nil"), Kind::Literal);
        assert_eq!(kind_of(&GO, src, 0, "// end"), Kind::Comment);
    }

    #[test]
    fn python_triple_quoted_and_hash_comment() {
        let src = "def f():\n    \"\"\"doc\n    def\n    \"\"\"\n    return None  # c";
        assert_eq!(kind_of(&PYTHON, src, 0, "def"), Kind::Keyword);
        assert_eq!(kind_of(&PYTHON, src, 2, "def"), Kind::String);
        assert_eq!(kind_of(&PYTHON, src, 4, "return"), Kind::Keyword);
        assert_eq!(kind_of(&PYTHON, src, 4, "None"), Kind::Literal);
        assert_eq!(kind_of(&PYTHON, src, 4, "# c"), Kind::Comment);
    }

    #[test]
    fn python_escaped_quote() {
        let src = r#"x = "a\"b" + y"#;
        assert_eq!(kind_of(&PYTHON, src, 0, "b\""), Kind::String);
        assert_eq!(kind_of(&PYTHON, src, 0, "y"), Kind::Plain);
    }

    #[test]
    fn shell_vars_and_comments() {
        let src = "if [ \"$HOME\" ]; then echo ${X} # note\nfi  # a#b";
        assert_eq!(kind_of(&SHELL, src, 0, "if"), Kind::Keyword);
        assert_eq!(kind_of(&SHELL, src, 0, "echo"), Kind::Builtin);
        assert_eq!(kind_of(&SHELL, src, 0, "${X}"), Kind::Builtin);
        assert_eq!(kind_of(&SHELL, src, 0, "# note"), Kind::Comment);
        assert_eq!(kind_of(&SHELL, src, 1, "# a#b"), Kind::Comment);
    }

    #[test]
    fn shell_hash_inside_word_is_not_a_comment() {
        let src = "echo a#b";
        assert_eq!(kind_of(&SHELL, src, 0, "#b"), Kind::Plain);
    }

    #[test]
    fn lua_comments_keywords_literals_and_operators() {
        let src = "local x = nil ~= y .. \"s\" -- note\n--[[ block\nstill ]] print(#t)";
        assert_eq!(detect(Some(Path::new("init.lua")), "").name, "Lua");
        assert_eq!(detect(None, "#!/usr/bin/env lua").name, "Lua");
        assert_eq!(kind_of(&LUA, src, 0, "local"), Kind::Keyword);
        assert_eq!(kind_of(&LUA, src, 0, "nil"), Kind::Literal);
        assert_eq!(kind_of(&LUA, src, 0, "~="), Kind::Operator);
        assert_eq!(kind_of(&LUA, src, 0, "\"s\""), Kind::String);
        assert_eq!(kind_of(&LUA, src, 0, "-- note"), Kind::Comment);
        assert_eq!(kind_of(&LUA, src, 1, "--[[ block"), Kind::Comment);
        assert_eq!(kind_of(&LUA, src, 2, "still ]]"), Kind::Comment, "block comments span lines");
        assert_eq!(kind_of(&LUA, src, 2, "print"), Kind::Builtin);
    }

    #[test]
    fn json_keys_values_literals() {
        let src = "{\"name\": \"x\", \"n\": -1.5e3, \"ok\": true, \"z\": null}";
        assert_eq!(kind_of(&JSON, src, 0, "\"name\""), Kind::Key);
        assert_eq!(kind_of(&JSON, src, 0, "\"x\""), Kind::String);
        assert_eq!(kind_of(&JSON, src, 0, "-1.5e3"), Kind::Number);
        assert_eq!(kind_of(&JSON, src, 0, "true"), Kind::Literal);
        assert_eq!(kind_of(&JSON, src, 0, "null"), Kind::Literal);
    }

    #[test]
    fn unterminated_string_does_not_panic_or_leak() {
        let src = "x = 'abc\ny = 1";
        let h = hl(&PYTHON, src);
        assert_eq!(h[0][4], Kind::String);
        assert_eq!(kind_of(&PYTHON, src, 1, "1"), Kind::Number);
    }

    #[test]
    fn unicode_columns_line_up_with_chars() {
        let src = "é = 'ü' # ñ";
        let h = hl(&PYTHON, src);
        assert_eq!(h[0].len(), src.chars().count());
        assert_eq!(h[0][4], Kind::String);
        assert_eq!(h[0][8], Kind::Comment);
    }

    #[test]
    fn plain_text_has_no_highlighting() {
        let h = hl(&PLAIN, "select 'x' -- 1");
        assert!(h[0].iter().all(|k| *k == Kind::Plain));
    }

    #[test]
    fn identifiers_with_digits_are_not_numbers() {
        let src = "x1 = 22";
        assert_eq!(kind_of(&PYTHON, src, 0, "1"), Kind::Plain);
        assert_eq!(kind_of(&PYTHON, src, 0, "22"), Kind::Number);
    }

    // ---- Part 3.4: operators, numbers, strings, comments, bracket depth ------------------

    /// Non-plain runs of the first line as (text, kind). Brackets are separate tokens (each has its own depth).
    fn toks(spec: &LangSpec, src: &str) -> Vec<(String, Kind)> {
        let line = src.split('\n').next().unwrap();
        let k = &hl(spec, src)[0];
        let chars: Vec<char> = line.chars().collect();
        let mut out: Vec<(String, Kind)> = vec![];
        let mut i = 0;
        while i < chars.len() {
            if k[i] == Kind::Plain {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && k[j] == k[i] && !matches!(k[i], Kind::Bracket(_) | Kind::BracketError) {
                j += 1;
            }
            out.push((chars[i..j].iter().collect(), k[i]));
            i = j;
        }
        out
    }

    fn has(t: &[(String, Kind)], text: &str, kind: Kind) -> bool {
        t.iter().any(|(s, k)| s == text && *k == kind)
    }

    #[test]
    fn multi_character_operators_are_single_tokens() {
        let t = toks(&GO, "if a == b && c != d || e <= f { x := y + 1; z -= 2 }");
        for op in ["==", "&&", "!=", "||", "<=", ":=", "+", "-="] {
            assert!(has(&t, op, Kind::Operator), "{op} in {t:?}");
        }
        let t = toks(&PYTHON, "x = a ** b // c -> d; y //= 2; z := 3");
        for op in ["=", "**", "//", "->", "//=", ":="] {
            assert!(has(&t, op, Kind::Operator), "{op} in {t:?}");
        }
        let t = toks(&SQL, "SELECT a || b, c::int, d <> e, f->>'k' FROM t WHERE x >= 1 AND y != 2");
        for op in ["||", "::", "<>", "->>", ">=", "!="] {
            assert!(has(&t, op, Kind::Operator), "{op} in {t:?}");
        }
        let t = toks(&SHELL, "a && b || c >> out; x=1");
        for op in ["&&", "||", ">>", "="] {
            assert!(has(&t, op, Kind::Operator), "{op} in {t:?}");
        }
    }

    #[test]
    fn longest_operator_wins() {
        let t = toks(&GO, "a <<= 2");
        assert!(has(&t, "<<=", Kind::Operator), "{t:?}");
        assert!(!has(&t, "<", Kind::Operator));
        let t = toks(&GO, "a &^= 2");
        assert!(has(&t, "&^=", Kind::Operator), "{t:?}");
    }

    #[test]
    fn json_has_no_operators() {
        let t = toks(&JSON, "{\"a\": 1, \"b\": [true]}");
        assert!(t.iter().all(|(_, k)| *k != Kind::Operator), "{t:?}");
    }

    #[test]
    fn operators_inside_strings_and_comments_are_not_operators() {
        let t = toks(&GO, "x := \"a == b\" // c && d");
        assert_eq!(t.iter().filter(|(_, k)| *k == Kind::Operator).count(), 1, "only := : {t:?}");
    }

    #[test]
    fn numbers_in_decimal_hex_exponent_and_other_forms() {
        for (lang, src, num) in [
            (&GO, "x := 42", "42"),
            (&GO, "x := 3.14", "3.14"),
            (&GO, "x := 0xDEADbeef", "0xDEADbeef"),
            (&GO, "x := 1_000_000", "1_000_000"),
            (&GO, "x := 0b1010", "0b1010"),
            (&GO, "x := 0o17", "0o17"),
            (&PYTHON, "x = 1e10", "1e10"),
            (&PYTHON, "x = 2.5E-3", "2.5E-3"),
            (&PYTHON, "x = 1e+5", "1e+5"),
            (&PYTHON, "x = 0XFF", "0XFF"),
            (&SQL, "SELECT 1.5e3", "1.5e3"),
            (&SQL, "SELECT 0x1F", "0x1F"),
            (&JSON, "[-12.5e-2]", "-12.5e-2"),
        ] {
            let t = toks(lang, src);
            assert!(has(&t, num, Kind::Number), "{src}: {t:?}");
        }
    }

    #[test]
    fn digits_inside_identifiers_are_not_numbers() {
        let t = toks(&PYTHON, "var2 = x1 + y_3");
        assert!(t.iter().all(|(_, k)| *k != Kind::Number), "{t:?}");
    }

    #[test]
    fn strings_with_escapes_single_and_double_quoted() {
        let t = toks(&PYTHON, r#"a = 'it\'s' + "say \"hi\"""#);
        assert!(has(&t, r"'it\'s'", Kind::String), "{t:?}");
        assert!(has(&t, r#""say \"hi\"""#, Kind::String), "{t:?}");
        let t = toks(&GO, r#"s := "a\\" + 'b'"#);
        assert!(has(&t, r#""a\\""#, Kind::String), "backslash-backslash closes the string: {t:?}");
        assert!(has(&t, "'b'", Kind::String));
    }

    #[test]
    fn unterminated_strings_end_at_the_line_end_and_do_not_leak() {
        let h = hl(&GO, "x := \"abc\ny := 1");
        assert_eq!(h[0][5], Kind::String);
        assert_eq!(h[1][2], Kind::Operator, "the next line is tokenised normally");
        let t = toks(&GO, "y := 1");
        assert!(has(&t, "1", Kind::Number));
        let h = hl(&SHELL, "echo 'abc\necho done");
        assert_eq!(h[1][0], Kind::Builtin, "next line is highlighted normally");
    }

    #[test]
    fn comment_syntax_per_language() {
        assert!(has(&toks(&GO, "x // c"), "// c", Kind::Comment));
        assert!(has(&toks(&GO, "x /* c */ y"), "/* c */", Kind::Comment));
        assert!(has(&toks(&PYTHON, "x # c"), "# c", Kind::Comment));
        assert!(has(&toks(&SQL, "x -- c"), "-- c", Kind::Comment));
        assert!(has(&toks(&SQL, "x /* c */ y"), "/* c */", Kind::Comment));
        assert!(has(&toks(&SHELL, "x # c"), "# c", Kind::Comment));
        assert!(toks(&JSON, "[1] // not a comment in JSON").iter().all(|(_, k)| *k != Kind::Comment));
        assert!(toks(&GO, "x # not a go comment").iter().all(|(_, k)| *k != Kind::Comment));
        assert!(toks(&PYTHON, "x // y").iter().all(|(_, k)| *k != Kind::Comment), "// is floor division in Python");
    }

    fn depths(spec: &LangSpec, src: &str) -> Vec<(char, Kind)> {
        let line = src.split('\n').next().unwrap();
        let k = &hl(spec, src)[0];
        line.chars()
            .zip(k.iter())
            .filter(|(c, k)| "()[]{}".contains(*c) && matches!(k, Kind::Bracket(_) | Kind::BracketError))
            .map(|(c, k)| (c, *k))
            .collect()
    }

    #[test]
    fn bracket_depth_increases_with_nesting_and_pairs_share_a_depth() {
        let d = depths(&GO, "f(a[b{c}])");
        let kinds: Vec<Kind> = d.iter().map(|(_, k)| *k).collect();
        assert_eq!(kinds, vec![Kind::Bracket(0), Kind::Bracket(1), Kind::Bracket(2), Kind::Bracket(2), Kind::Bracket(1), Kind::Bracket(0)]);
        let d = depths(&PYTHON, "(a)(b)");
        assert_eq!(d.iter().map(|(_, k)| *k).collect::<Vec<_>>(), vec![Kind::Bracket(0); 4], "siblings are both depth 0");
    }

    #[test]
    fn mismatched_and_unmatched_brackets_are_flagged() {
        let d = depths(&GO, "f(a]");
        assert_eq!(d[1].1, Kind::BracketError, "wrong closer: {d:?}");
        assert_eq!(d[0].1, Kind::BracketError, "the opener it failed to close is flagged at end of file");
        let d = depths(&GO, "a)");
        assert_eq!(d[0].1, Kind::BracketError, "closer without opener");
        let d = depths(&GO, "(((");
        assert!(d.iter().all(|(_, k)| *k == Kind::BracketError), "unclosed openers: {d:?}");
        let d = depths(&JSON, "{\"a\": [1, 2}");
        assert!(d.iter().any(|(_, k)| *k == Kind::BracketError));
    }

    #[test]
    fn brackets_in_strings_and_comments_are_ignored() {
        let d = depths(&GO, "f(\"(((\") // )))");
        assert_eq!(d.len(), 2, "only the real pair counts: {d:?}");
        assert!(d.iter().all(|(_, k)| *k == Kind::Bracket(0)));
        let d = depths(&PYTHON, "x = '[' # ]");
        assert!(d.is_empty(), "{d:?}");
        let d = depths(&SQL, "SELECT '(' /* ) */ , (1)");
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn brackets_match_across_lines_and_unclosed_openers_are_flagged_at_eof() {
        let h = hl(&GO, "f(\n  g(\n  )\n)");
        assert_eq!(h[0][1], Kind::Bracket(0));
        assert_eq!(h[1][3], Kind::Bracket(1));
        assert_eq!(h[2][2], Kind::Bracket(1));
        assert_eq!(h[3][0], Kind::Bracket(0));
        let h = hl(&GO, "f(\n  g(\n  )");
        assert_eq!(h[0][1], Kind::BracketError, "outer paren never closed");
        assert_eq!(h[1][3], Kind::Bracket(1));
    }

    #[test]
    fn shell_does_not_flag_case_pattern_parens() {
        let h = hl(&SHELL, "case $x in\n  a) echo a;;\nesac");
        assert!(h.iter().flatten().all(|k| *k != Kind::BracketError));
    }

    #[test]
    fn plain_text_has_no_brackets_or_operators() {
        let h = hl(&PLAIN, "(a == b]");
        assert!(h[0].iter().all(|k| *k == Kind::Plain));
    }
}
