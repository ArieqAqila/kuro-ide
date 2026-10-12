//! Turning a validated extension manifest into language modes the editor can use.
//!
//! An extension language becomes an ordinary [`LangSpec`] (so the generic tokenizer, bracket colours, the picker and `:setlang` all apply)
//! plus [`ExtData`]: the regex rules, the declared-name rules, the names read from the user's own files, and the colour roles the
//! extension defines. Strings are leaked once per process; the manifest limits keep that small.

use super::manifest::{CORE_ROLES, LanguageDef, Manifest};
use crate::syntax::{BASE, Kind, LangSpec};
use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Core(Kind),
    /// Index into [`ExtData::roles`].
    Own(u8),
}

impl Role {
    pub fn kind(self) -> Kind {
        match self {
            Role::Core(k) => k,
            Role::Own(i) => Kind::Ext(i),
        }
    }
}

#[derive(Debug)]
pub struct RoleDef {
    pub name: String,
    /// Default palette slot (0-15) or -1 for the terminal's default text colour.
    pub default_slot: i8,
}

#[derive(Debug)]
pub struct CompiledRule {
    pub pattern: Regex,
    pub role: Role,
    pub in_section: Option<Regex>,
    pub end_section: Option<Regex>,
}

#[derive(Debug)]
pub struct CompiledDecl {
    pub section: Regex,
    pub name: Regex,
    pub role: Role,
}

#[derive(Debug)]
pub struct NamesSource {
    pub setting: String,
    pub parser: String,
    pub role: Role,
}

#[derive(Debug)]
pub struct ExtData {
    /// The extension this language comes from (the `[ext.<name>]` table in the user's config).
    pub ext_name: String,
    pub roles: Vec<RoleDef>,
    pub rules: Vec<CompiledRule>,
    pub declares: Vec<CompiledDecl>,
    pub names_from: Vec<NamesSource>,
    pub filename_globs: Vec<Regex>,
    pub first_line: Option<Regex>,
    /// Lines that start a new section (ends the previous one for `declares` and rules).
    pub section_header: Regex,
    /// Names read from the user's own files (`names_from`), by exact name: filled in by the app after start-up.
    pub external: RwLock<HashMap<String, Role>>,
}

impl ExtData {
    pub fn role_index(&self, name: &str) -> Option<u8> {
        self.roles.iter().position(|r| r.name == name).map(|i| i as u8)
    }
}

const DEFAULT_SECTION: &str = r"^[A-Z][A-Z0-9_]*(\(|\s*$)";

fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn leak_list(v: &[String]) -> &'static [&'static str] {
    let items: Vec<&'static str> = v.iter().map(|s| leak_str(s)).collect();
    Box::leak(items.into_boxed_slice())
}

fn rx(p: &str) -> Regex {
    RegexBuilder::new(p).size_limit(super::manifest::REGEX_SIZE_LIMIT).build().expect("validated by the manifest check")
}

fn core_kind(role: &str) -> Option<Kind> {
    Some(match role {
        "keyword" => Kind::Keyword,
        "builtin" => Kind::Builtin,
        "literal" => Kind::Literal,
        "string" => Kind::String,
        "number" => Kind::Number,
        "comment" => Kind::Comment,
        "key" => Kind::Key,
        "operator" => Kind::Operator,
        _ => return None,
    })
}

fn build_language(ext_name: &str, d: &LanguageDef) -> &'static LangSpec {
    let roles: Vec<RoleDef> = d.roles.iter().map(|(n, s)| RoleDef { name: n.clone(), default_slot: *s as i8 }).collect();
    let role_of = |name: &str| -> Role {
        match core_kind(name) {
            Some(k) => {
                debug_assert!(CORE_ROLES.contains(&name));
                Role::Core(k)
            }
            None => Role::Own(roles.iter().position(|r| r.name == name).expect("validated") as u8),
        }
    };
    let data = ExtData {
        ext_name: ext_name.to_string(),
        rules: d
            .rule
            .iter()
            .map(|r| CompiledRule {
                pattern: rx(&r.pattern),
                role: role_of(&r.role),
                in_section: r.in_section.as_deref().map(rx),
                end_section: r.end_section.as_deref().map(rx),
            })
            .collect(),
        declares: d.declares.iter().map(|x| CompiledDecl { section: rx(&x.section), name: rx(&x.name), role: role_of(&x.role) }).collect(),
        names_from: d
            .names_from
            .iter()
            .map(|n| NamesSource { setting: n.setting.clone(), parser: n.parser.clone(), role: role_of(&n.role) })
            .collect(),
        filename_globs: d
            .filename_globs
            .iter()
            .map(|g| rx(&super::glob_to_regex(g)))
            .chain(d.filename_regex.iter().map(|r| rx(r)))
            .collect(),
        first_line: d.first_line_regex.as_deref().map(rx),
        section_header: rx(DEFAULT_SECTION),
        external: RwLock::new(HashMap::new()),
        roles,
    };
    let spec = LangSpec {
        name: leak_str(&d.name),
        extensions: leak_list(&d.extensions),
        filenames: leak_list(&d.filenames),
        shebangs: leak_list(&d.shebangs),
        keywords: leak_list(&d.keywords),
        builtins: leak_list(&d.builtins),
        literals: leak_list(&d.literals),
        line_comments: leak_list(&d.line_comments),
        block_comment: d.block_comment.as_ref().map(|[a, b]| (leak_str(a), leak_str(b))),
        string_quotes: Box::leak(d.string_quotes.iter().filter_map(|q| q.chars().next()).collect::<Vec<char>>().into_boxed_slice()),
        multiline_strings: d.multiline_strings,
        case_insensitive: d.case_insensitive,
        operators: leak_list(&d.operators),
        brackets: d.brackets,
        bracket_errors: d.bracket_errors,
        aliases: leak_list(&d.aliases),
        ext: Some(Box::leak(Box::new(data))),
        ..BASE
    };
    Box::leak(Box::new(spec))
}

/// The language modes of one extension.
pub fn build(m: &Manifest) -> Vec<&'static LangSpec> {
    m.language.iter().map(|d| build_language(&m.name, d)).collect()
}

/// Fill in the names read from the user's files (see `names_from`), replacing any earlier ones.
pub fn set_external_names(spec: &LangSpec, names: HashMap<String, Role>) {
    if let Some(e) = spec.ext {
        *e.external.write().unwrap() = names;
    }
}
