//! The manifest of an extension (`kuro-ext.toml`): parsing and validation.
//!
//! An extension is **data**: language definitions, nothing that kuro executes. Everything is checked here, with messages that name the
//! offending field, before anything is installed or loaded. See docs/design/extensions.md and docs/EXTENSIONS.md.

use regex::RegexBuilder;
use serde::Deserialize;
use std::collections::BTreeMap;

/// The manifest schema version this kuro understands.
pub const FORMAT: u32 = 1;
pub const MANIFEST_FILE: &str = "kuro-ext.toml";

pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
pub const MAX_LANGUAGES: usize = 16;
pub const MAX_LIST: usize = 2000;
pub const MAX_RULES: usize = 100;
pub const MAX_PATTERN: usize = 1024;
pub const MAX_ROLES: usize = 16;
pub const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// The colour roles every language has; an extension can use them or declare its own.
pub const CORE_ROLES: &[&str] = &["keyword", "builtin", "literal", "string", "number", "comment", "key", "operator"];
/// The built-in readers of `names_from` (an extension can pick one, never supply code).
pub const PARSERS: &[&str] = &["c-struct-members", "c-negative-defines"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub name: String,
    pub version: String,
    /// Range of kuro versions this works with, e.g. `">=0.2, <0.3"`.
    pub kuro: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub language: Vec<LanguageDef>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LanguageDef {
    pub name: String,
    pub aliases: Vec<String>,
    pub extensions: Vec<String>,
    pub filenames: Vec<String>,
    /// `*`, `?` and `[a-z]` patterns matched against the file name (`sct[0-9]*`).
    pub filename_globs: Vec<String>,
    /// Full regular expressions matched against the whole file name (when a glob is not precise enough).
    pub filename_regex: Vec<String>,
    pub shebangs: Vec<String>,
    /// A regular expression tried against the first line of the file.
    pub first_line_regex: Option<String>,
    pub keywords: Vec<String>,
    pub builtins: Vec<String>,
    pub literals: Vec<String>,
    pub line_comments: Vec<String>,
    pub block_comment: Option<[String; 2]>,
    /// One character each.
    pub string_quotes: Vec<String>,
    /// A string may continue on the next line.
    pub multiline_strings: bool,
    pub operators: Vec<String>,
    pub brackets: bool,
    pub bracket_errors: bool,
    pub case_insensitive: bool,
    /// Own colour roles: name -> default ANSI slot (0-15) or -1 for the terminal's default text colour.
    pub roles: BTreeMap<String, i64>,
    pub rule: Vec<Rule>,
    pub declares: Vec<Declares>,
    pub names_from: Vec<NamesFrom>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub pattern: String,
    pub role: String,
    /// Only inside the section that starts at a line matching this and ends at the next line matching `end_section`
    /// (default: the next line that starts a new section, `^[A-Z_]+(\(|$)`).
    #[serde(default)]
    pub in_section: Option<String>,
    #[serde(default)]
    pub end_section: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declares {
    /// Lines after a line matching this, up to the next section header, are declarations.
    pub section: String,
    /// Matched against each such line; capture group 1 is the declared name.
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamesFrom {
    /// The key under `[ext.<name>]` in the user's config that lists the files.
    pub setting: String,
    pub parser: String,
    pub role: String,
}

/// Parse and validate. Every problem found is returned (not just the first), each naming its field.
pub fn parse(text: &str, kuro_version: &str) -> Result<Manifest, Vec<String>> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(vec![format!("{MANIFEST_FILE} is {} KiB; the limit is {} KiB", text.len() / 1024, MAX_MANIFEST_BYTES / 1024)]);
    }
    let m: Manifest = toml::from_str(text).map_err(|e| vec![format!("{MANIFEST_FILE} is not valid: {}", e.message().trim())])?;
    let errs = validate(&m, kuro_version);
    if errs.is_empty() { Ok(m) } else { Err(errs) }
}

pub fn validate_name(s: &str) -> bool {
    is_name(s)
}

fn is_name(s: &str) -> bool {
    (1..=40).contains(&s.len())
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_role_name(s: &str) -> bool {
    (1..=24).contains(&s.len())
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_language_name(s: &str) -> bool {
    (1..=30).contains(&s.len())
        && !s.starts_with(' ')
        && !s.ends_with(' ')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "+#._ -".contains(c))
}

fn regex_error(field: &str, pat: &str) -> Option<String> {
    if pat.len() > MAX_PATTERN {
        return Some(format!("{field}: the pattern is {} bytes; the limit is {MAX_PATTERN}", pat.len()));
    }
    RegexBuilder::new(pat)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .err()
        .map(|e| format!("{field}: not a valid pattern ({})", e.to_string().lines().last().unwrap_or("").trim()))
}

pub fn validate(m: &Manifest, kuro_version: &str) -> Vec<String> {
    let mut e = Vec::new();
    if m.format != FORMAT {
        e.push(format!("format = {} is not supported (this kuro understands format {FORMAT})", m.format));
    }
    if !is_name(&m.name) {
        e.push(format!("name `{}` must be 1-40 characters: lower-case letters, digits and `-`", m.name));
    }
    if m.version.trim().is_empty() || m.version.len() > 40 {
        e.push("version must be a short version number such as \"0.1.0\"".into());
    }
    match super::version::satisfies(&m.kuro, kuro_version) {
        Ok(true) => {}
        Ok(false) => e.push(format!("this extension needs kuro {} but this is kuro {kuro_version}", m.kuro)),
        Err(msg) => e.push(format!("kuro: {msg}")),
    }
    if m.description.len() > 500 || m.license.len() > 100 {
        e.push("description (500 bytes) or license (100 bytes) is too long".into());
    }
    if m.language.is_empty() {
        e.push("an extension must define at least one [[language]]".into());
    }
    if m.language.len() > MAX_LANGUAGES {
        e.push(format!("{} languages; the limit is {MAX_LANGUAGES}", m.language.len()));
    }
    let core_names: Vec<String> =
        crate::syntax::LANGS.iter().map(|l| l.name.to_ascii_lowercase()).chain(["plain text".to_string()]).collect();
    let mut seen = Vec::new();
    for (i, l) in m.language.iter().enumerate() {
        let at = |f: &str| format!("language[{i}] ({}): {f}", l.name);
        if !is_language_name(&l.name) {
            e.push(format!("language[{i}]: name `{}` must be 1-30 characters (letters, digits and `+#._ -`)", l.name));
        }
        let lower = l.name.to_ascii_lowercase();
        if core_names.contains(&lower) {
            e.push(at(&format!("`{}` is the name of a built-in language mode", l.name)));
        }
        if seen.contains(&lower) {
            e.push(at("the name is used twice in this extension"));
        }
        seen.push(lower);
        for (field, list) in [
            ("keywords", &l.keywords),
            ("builtins", &l.builtins),
            ("literals", &l.literals),
            ("operators", &l.operators),
            ("extensions", &l.extensions),
            ("filenames", &l.filenames),
            ("filename_globs", &l.filename_globs),
            ("filename_regex", &l.filename_regex),
            ("shebangs", &l.shebangs),
            ("aliases", &l.aliases),
            ("line_comments", &l.line_comments),
        ] {
            if list.len() > MAX_LIST {
                e.push(at(&format!("{field} has {} entries; the limit is {MAX_LIST}", list.len())));
            }
            if list.iter().any(|w| w.is_empty() || w.len() > 100 || w.chars().any(|c| c.is_control())) {
                e.push(at(&format!("{field}: entries must be 1-100 characters without control characters")));
            }
        }
        if l.string_quotes.iter().any(|q| q.chars().count() != 1) {
            e.push(at("string_quotes: each entry must be exactly one character"));
        }
        if let Some([a, b]) = &l.block_comment
            && (a.is_empty() || b.is_empty())
        {
            e.push(at("block_comment: both the opener and the closer are needed"));
        }
        if l.extensions.iter().any(|x| x.starts_with('.') || x.contains(['/', '\\'])) {
            e.push(at("extensions: write them without the dot and without slashes (\"var\", not \".var\")"));
        }
        if l.filenames.iter().chain(&l.filename_globs).any(|x| x.contains(['/', '\\']) || x == "." || x == "..") {
            e.push(at("filenames / filename_globs: a file name, not a path"));
        }
        for g in &l.filename_globs {
            if let Some(m) = regex_error(&at("filename_globs"), &super::glob_to_regex(g)) {
                e.push(m);
            }
        }
        for r in &l.filename_regex {
            e.extend(regex_error(&at("filename_regex"), r));
        }
        if let Some(p) = &l.first_line_regex
            && let Some(m) = regex_error(&at("first_line_regex"), p)
        {
            e.push(m);
        }
        // roles
        if l.roles.len() > MAX_ROLES {
            e.push(at(&format!("{} roles; the limit is {MAX_ROLES}", l.roles.len())));
        }
        for (name, slot) in &l.roles {
            if !is_role_name(name) {
                e.push(at(&format!("role name `{name}` must be lower-case letters, digits and `_`, starting with a letter")));
            }
            if CORE_ROLES.contains(&name.as_str()) {
                e.push(at(&format!("role `{name}` is a core role and cannot be redefined")));
            }
            if !(-1..=15).contains(slot) {
                e.push(at(&format!(
                    "role `{name}` = {slot}: only the terminal's palette slots 0-15, or -1 for the default text colour, are allowed (no RGB, no 256-colour)"
                )));
            }
        }
        let known_role = |r: &str| CORE_ROLES.contains(&r) || l.roles.contains_key(r);
        let role_err = |what: String, r: &str| {
            (!known_role(r)).then(|| {
                at(&format!("{what}: unknown role `{r}` (core roles: {}; or declare it under [language.roles])", CORE_ROLES.join(", ")))
            })
        };
        if l.rule.len() > MAX_RULES {
            e.push(at(&format!("{} rules; the limit is {MAX_RULES}", l.rule.len())));
        }
        for (j, r) in l.rule.iter().enumerate() {
            e.extend(regex_error(&at(&format!("rule[{j}].pattern")), &r.pattern));
            e.extend(role_err(format!("rule[{j}].role"), &r.role));
            for (f, p) in [("in_section", &r.in_section), ("end_section", &r.end_section)] {
                if let Some(p) = p {
                    e.extend(regex_error(&at(&format!("rule[{j}].{f}")), p));
                }
            }
        }
        for (j, d) in l.declares.iter().enumerate() {
            e.extend(regex_error(&at(&format!("declares[{j}].section")), &d.section));
            e.extend(regex_error(&at(&format!("declares[{j}].name")), &d.name));
            e.extend(role_err(format!("declares[{j}].role"), &d.role));
            if RegexBuilder::new(&d.name).build().is_ok_and(|re| re.captures_len() < 2) {
                e.push(at(&format!("declares[{j}].name: the pattern needs a capture group (the declared name)")));
            }
        }
        for (j, n) in l.names_from.iter().enumerate() {
            if !PARSERS.contains(&n.parser.as_str()) {
                e.push(at(&format!(
                    "names_from[{j}].parser: `{}` is not a built-in reader (choose one of: {})",
                    n.parser,
                    PARSERS.join(", ")
                )));
            }
            if !is_role_name(&n.setting) || n.setting == "colors" {
                e.push(at(&format!("names_from[{j}].setting: `{}` must be a plain key name (not `colors`)", n.setting)));
            }
            e.extend(role_err(format!("names_from[{j}].role"), &n.role));
        }
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
format = 1
name = "demo"
version = "0.1.0"
kuro = ">=0.1, <0.2"

[[language]]
name = "Demo"
extensions = ["dm"]
keywords = ["if", "then"]
line_comments = ["//"]
string_quotes = ["\""]

[language.roles]
mine = 13

[[language.rule]]
pattern = '^[A-Z_]+\('
role = "keyword"

[[language.declares]]
section = '^VARS'
name = '^(v[a-z]+)'
role = "mine"
"#;

    fn errs(text: &str) -> String {
        parse(text, "0.1.0").unwrap_err().join("\n")
    }

    #[test]
    fn a_good_manifest_parses() {
        let m = parse(GOOD, "0.1.0").unwrap();
        assert_eq!((m.name.as_str(), m.language.len(), m.language[0].roles["mine"]), ("demo", 1, 13));
    }

    #[test]
    fn broken_toml_and_unknown_keys_are_rejected_with_the_key_named() {
        assert!(errs("this is not = = toml").contains("not valid"));
        let e = errs(&GOOD.replace("version = \"0.1.0\"", "version = \"0.1.0\"\nmystery = 1"));
        assert!(e.contains("mystery"), "{e}");
        let e = errs(&GOOD.replace("[[language.rule]]", "[[language.rule]]\ncolor = \"#ff0000\""));
        assert!(e.contains("color"), "an RGB-looking field is an unknown field: {e}");
        assert!(errs(&GOOD.replace("name = \"demo\"\n", "")).contains("name"));
    }

    #[test]
    fn format_name_and_kuro_range_are_checked() {
        assert!(errs(&GOOD.replace("format = 1", "format = 2")).contains("format = 2 is not supported"));
        assert!(errs(&GOOD.replace("name = \"demo\"", "name = \"Bad Name\"")).contains("name `Bad Name`"));
        assert!(errs(&GOOD.replace("name = \"demo\"", "name = \"../x\"")).contains("name `../x`"));
        let e = errs(&GOOD.replace(">=0.1, <0.2", ">=0.2"));
        assert!(e.contains("needs kuro >=0.2 but this is kuro 0.1.0"), "{e}");
        assert!(errs(&GOOD.replace(">=0.1, <0.2", "banana")).contains("kuro:"));
    }

    #[test]
    fn roles_are_palette_slots_never_colours() {
        assert!(errs(&GOOD.replace("mine = 13", "mine = 16")).contains("palette slots 0-15"));
        assert!(errs(&GOOD.replace("mine = 13", "mine = 200")).contains("no RGB, no 256-colour"));
        assert!(errs(&GOOD.replace("mine = 13", "mine = \"#ff0000\"")).contains("not valid"));
        assert!(errs(&GOOD.replace("mine = 13", "keyword = 3")).contains("core role"));
        assert!(errs(&GOOD.replace("role = \"keyword\"", "role = \"nonexistent\"")).contains("unknown role `nonexistent`"));
        assert!(parse(&GOOD.replace("mine = 13", "mine = -1"), "0.1.0").is_ok());
    }

    #[test]
    fn patterns_and_limits_are_enforced() {
        assert!(errs(&GOOD.replace("'^[A-Z_]+\\('", "'(unclosed'")).contains("not a valid pattern"));
        let huge = format!("'{}'", "a".repeat(2000));
        assert!(errs(&GOOD.replace("'^[A-Z_]+\\('", &huge)).contains("limit is 1024"));
        assert!(errs(&GOOD.replace("'^(v[a-z]+)'", "'^v[a-z]+'")).contains("capture group"));
        let many = "x".repeat(MAX_MANIFEST_BYTES + 1);
        assert!(errs(&many).contains("KiB"));
        let list = (0..MAX_LIST + 1).map(|i| format!("\"w{i}\"")).collect::<Vec<_>>().join(", ");
        assert!(errs(&GOOD.replace("[\"if\", \"then\"]", &format!("[{list}]"))).contains("keywords has"));
    }

    #[test]
    fn languages_cannot_take_a_built_in_name_or_repeat() {
        assert!(errs(&GOOD.replace("name = \"Demo\"", "name = \"Python\"")).contains("built-in language mode"));
        let two = format!("{GOOD}\n[[language]]\nname = \"demo\"\n");
        assert!(errs(&two).contains("used twice"));
        assert!(errs(&GOOD.replace("extensions = [\"dm\"]", "extensions = [\".dm\"]")).contains("without the dot"));
        assert!(errs(&GOOD.replace("string_quotes = [\"\\\"\"]", "string_quotes = [\"ab\"]")).contains("exactly one character"));
    }

    #[test]
    fn names_from_must_use_a_built_in_reader() {
        let nf = format!("{GOOD}\n[[language.names_from]]\nsetting = \"headers\"\nparser = \"run-this-script\"\nrole = \"mine\"\n");
        assert!(errs(&nf).contains("not a built-in reader"));
        let ok = nf.replace("run-this-script", "c-struct-members");
        assert!(parse(&ok, "0.1.0").is_ok());
        assert!(errs(&ok.replace("headers", "colors")).contains("plain key name"));
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let e = parse(&GOOD.replace("format = 1", "format = 9").replace("mine = 13", "mine = 99"), "0.1.0").unwrap_err();
        assert!(e.len() >= 2, "{e:?}");
    }
}
