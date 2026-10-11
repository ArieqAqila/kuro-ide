//! Configuration loading.
//!
//! Every option lives in one of the section structs below. `option_keys()`
//! derives the list of valid `section.key` names from `Config::default()`, so
//! code is the single source of truth; the docs check test compares it with
//! `docs/CUSTOMIZATION.md`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    /// vi-style modes. When false the editor is always in "insert" (VS Code feel).
    pub modal: bool,
    pub tab_width: usize,
    pub expand_tab: bool,
    pub line_numbers: bool,
    pub relative_numbers: bool,
    pub mouse: bool,
    pub double_click_ms: u64,
    pub scroll_lines: usize,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            modal: true,
            tab_width: 4,
            expand_tab: true,
            line_numbers: true,
            relative_numbers: false,
            mouse: true,
            double_click_ms: 400,
            scroll_lines: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardConfig {
    /// auto | wl-clipboard | xclip | xsel | osc52
    pub backend: String,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self { backend: "auto".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FindConfig {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SqlConfig {
    /// split | tab
    pub result_view: String,
    pub table_max_width: usize,
    pub table_max_rows: usize,
    pub column_max_width: usize,
    pub row_limit: usize,
    /// Empty = `<config dir>/connections.toml`.
    pub connections_file: String,
}

impl Default for SqlConfig {
    fn default() -> Self {
        Self {
            result_view: "split".into(),
            table_max_width: 120,
            table_max_rows: 25,
            column_max_width: 40,
            row_limit: 10_000,
            connections_file: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub syntax_highlight: bool,
    /// Push the kitty keyboard protocol flags so Ctrl+Shift+Z etc. are distinguishable.
    pub kitty_keyboard: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { syntax_highlight: true, kitty_keyboard: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub editor: EditorConfig,
    pub clipboard: ClipboardConfig,
    pub find: FindConfig,
    pub sql: SqlConfig,
    pub ui: UiConfig,
}

/// Environment variables the program reads (documented in docs/CUSTOMIZATION.md).
pub const ENV_VARS: &[&str] = &["KURO_CONFIG_DIR"];

pub const CLIPBOARD_BACKENDS: &[&str] = &["auto", "wl-clipboard", "xclip", "xsel", "osc52"];
pub const RESULT_VIEWS: &[&str] = &["split", "tab"];

/// Flattened `section.key -> default` map, derived from `Config::default()`.
pub fn option_defaults() -> BTreeMap<String, toml::Value> {
    let v = toml::Value::try_from(Config::default()).expect("default config serialises");
    let mut out = BTreeMap::new();
    if let toml::Value::Table(sections) = v {
        for (s, body) in sections {
            if let toml::Value::Table(t) = body {
                for (k, val) in t {
                    out.insert(format!("{s}.{k}"), val);
                }
            }
        }
    }
    out
}

pub fn option_keys() -> Vec<String> {
    option_defaults().into_keys().collect()
}

fn section<T: DeserializeOwned + Default>(table: &toml::Table, name: &str, warnings: &mut Vec<String>) -> T {
    match table.get(name) {
        None => T::default(),
        Some(v) => match v.clone().try_into::<T>() {
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!("[{name}]: invalid value ({}); using defaults for this section", e.message()));
                T::default()
            }
        },
    }
}

impl Config {
    /// Parse TOML text. Never fails: problems become warnings and defaults.
    pub fn from_toml(text: &str) -> (Config, Vec<String>) {
        let mut warnings = Vec::new();
        let table: toml::Table = match text.parse() {
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!("config is not valid TOML ({}); using defaults", e.message()));
                return (Config::default(), warnings);
            }
        };
        let known = option_defaults();
        for (sec, body) in &table {
            let prefix = format!("{sec}.");
            if !known.keys().any(|k| k.starts_with(&prefix)) {
                warnings.push(format!("unknown config section [{sec}] ignored"));
                continue;
            }
            match body {
                toml::Value::Table(t) => {
                    for k in t.keys() {
                        if !known.contains_key(&format!("{sec}.{k}")) {
                            warnings.push(format!("unknown config key `{sec}.{k}` ignored"));
                        }
                    }
                }
                _ => warnings.push(format!("`{sec}` should be a table; ignored")),
            }
        }
        let mut cfg = Config {
            editor: section(&table, "editor", &mut warnings),
            clipboard: section(&table, "clipboard", &mut warnings),
            find: section(&table, "find", &mut warnings),
            sql: section(&table, "sql", &mut warnings),
            ui: section(&table, "ui", &mut warnings),
        };
        cfg.validate(&mut warnings);
        (cfg, warnings)
    }

    fn validate(&mut self, warnings: &mut Vec<String>) {
        if !CLIPBOARD_BACKENDS.contains(&self.clipboard.backend.as_str()) {
            warnings
                .push(format!("clipboard.backend = {:?} is not one of {:?}; using \"auto\"", self.clipboard.backend, CLIPBOARD_BACKENDS));
            self.clipboard.backend = "auto".into();
        }
        if !RESULT_VIEWS.contains(&self.sql.result_view.as_str()) {
            warnings.push(format!("sql.result_view = {:?} is not one of {:?}; using \"split\"", self.sql.result_view, RESULT_VIEWS));
            self.sql.result_view = "split".into();
        }
        let d = Config::default();
        macro_rules! at_least_one {
            ($sec:ident . $key:ident) => {
                if self.$sec.$key == 0 {
                    warnings.push(format!("{}.{} must be at least 1; using {}", stringify!($sec), stringify!($key), d.$sec.$key));
                    self.$sec.$key = d.$sec.$key;
                }
            };
        }
        at_least_one!(editor.tab_width);
        at_least_one!(editor.scroll_lines);
        at_least_one!(sql.table_max_width);
        at_least_one!(sql.table_max_rows);
        at_least_one!(sql.column_max_width);
        at_least_one!(sql.row_limit);
    }

    /// Load from a file; a missing file is not an error (defaults, no warnings).
    pub fn load(path: &Path) -> (Config, Vec<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => Config::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), vec![]),
            Err(e) => (Config::default(), vec![format!("cannot read {}: {e}; using defaults", path.display())]),
        }
    }
}

/// `$KURO_CONFIG_DIR`, else `$XDG_CONFIG_HOME/kuro`, else `~/.config/kuro`.
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("KURO_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(d).join("kuro");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config").join("kuro")
}

pub fn default_config_path() -> PathBuf {
    config_dir().join("config.toml")
}

impl SqlConfig {
    pub fn connections_path(&self) -> PathBuf {
        if self.connections_file.is_empty() { config_dir().join("connections.toml") } else { PathBuf::from(&self.connections_file) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_is_defaults() {
        let (c, w) = Config::from_toml("");
        assert_eq!(c, Config::default());
        assert!(w.is_empty());
    }

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert!(c.editor.modal);
        assert!(!c.editor.relative_numbers, "line numbers are absolute by default; relative is opt-in");
        assert_eq!(c.sql.table_max_width, 120);
        assert_eq!(c.sql.table_max_rows, 25);
    }

    #[test]
    fn overrides_apply() {
        let (c, w) = Config::from_toml("[editor]\nmodal = false\ntab_width = 2\n[sql]\ntable_max_rows = 10\n");
        assert!(!c.editor.modal);
        assert_eq!(c.editor.tab_width, 2);
        assert_eq!(c.sql.table_max_rows, 10);
        assert!(w.is_empty());
    }

    #[test]
    fn unknown_keys_warn_not_crash() {
        let (c, w) = Config::from_toml("[editor]\nmodl = false\nmodal = false\n[nope]\nx = 1\n");
        assert!(!c.editor.modal, "known keys still apply");
        assert!(w.iter().any(|m| m.contains("editor.modl")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("[nope]")), "{w:?}");
    }

    #[test]
    fn wrong_type_warns_and_falls_back_for_that_section() {
        let (c, w) = Config::from_toml("[editor]\ntab_width = \"wide\"\n[find]\nregex = true\n");
        assert_eq!(c.editor, EditorConfig::default());
        assert!(c.find.regex, "other sections unaffected");
        assert!(w.iter().any(|m| m.contains("[editor]")), "{w:?}");
    }

    #[test]
    fn invalid_toml_warns_and_uses_defaults() {
        let (c, w) = Config::from_toml("[editor\nmodal = ");
        assert_eq!(c, Config::default());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn bad_enum_and_zero_values_are_repaired() {
        let (c, w) = Config::from_toml("[clipboard]\nbackend = \"pigeon\"\n[sql]\nresult_view = \"x\"\ntable_max_rows = 0\n");
        assert_eq!(c.clipboard.backend, "auto");
        assert_eq!(c.sql.result_view, "split");
        assert_eq!(c.sql.table_max_rows, 25);
        assert_eq!(w.len(), 3, "{w:?}");
    }

    #[test]
    fn missing_file_gives_defaults_silently() {
        let (c, w) = Config::load(Path::new("/nonexistent/kuro/config.toml"));
        assert_eq!(c, Config::default());
        assert!(w.is_empty());
    }

    #[test]
    fn committed_default_config_equals_builtin_defaults() {
        let text = include_str!("../config/default/config.toml");
        let (c, w) = Config::from_toml(text);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c, Config::default());
        // every option appears explicitly in the committed file
        let table: toml::Table = text.parse().unwrap();
        for key in option_keys() {
            let (s, k) = key.split_once('.').unwrap();
            assert!(table.get(s).and_then(|t| t.get(k)).is_some(), "config/default/config.toml lacks {key}");
        }
    }
}
