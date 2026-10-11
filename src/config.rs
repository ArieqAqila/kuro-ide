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
    /// Display width of a tab character.
    pub tab_width: usize,
    /// tabs | spaces
    pub indent_style: String,
    /// Columns per indent step when indenting with spaces (and per outdent).
    pub indent_size: usize,
    /// Backspace/Delete remove a whole run of whitespace in one keypress.
    pub smart_whitespace_delete: bool,
    /// Suggest words already in the file while typing.
    pub word_suggestions: bool,
    /// Characters typed before suggestions appear.
    pub suggest_min_chars: usize,
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
            tab_width: 8,
            indent_style: "tabs".into(),
            indent_size: 8,
            smart_whitespace_delete: true,
            word_suggestions: true,
            suggest_min_chars: 2,
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
    /// Key that runs the selection / current statement where Ctrl+Enter cannot be sent (see docs).
    pub run_fallback_key: String,
    /// Key that runs only the current line.
    pub run_line_key: String,
    /// In a SQL buffer, refuse edits (and Insert mode) until a database connection exists.
    pub block_editing_when_disconnected: bool,
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
            run_fallback_key: "alt+enter".into(),
            run_line_key: "alt+l".into(),
            block_editing_when_disconnected: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub syntax_highlight: bool,
    /// auto | on | off: use the kitty keyboard protocol so Ctrl+Enter, Shift+Tab etc. are distinguishable.
    pub kitty_keyboard: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { syntax_highlight: true, kitty_keyboard: "auto".into() }
    }
}

/// Colour roles mapped to ANSI palette slots: 0-15, or -1 for "default foreground".
/// The actual colours come from the terminal's theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorsConfig {
    pub keyword: i8,
    pub builtin: i8,
    pub literal: i8,
    pub string: i8,
    pub number: i8,
    pub operator: i8,
    /// -1 draws comments dim and italic in the default foreground.
    pub comment: i8,
    pub key: i8,
    /// Unmatched / mismatched brackets (also drawn in reverse video).
    pub bracket_error: i8,
    /// Colours cycled through by bracket nesting depth.
    pub bracket_depth: Vec<i8>,
}

impl Default for ColorsConfig {
    fn default() -> Self {
        Self {
            keyword: 5,
            builtin: 4,
            literal: 3,
            string: 2,
            number: 6,
            operator: 1,
            comment: -1,
            key: 4,
            bracket_error: 1,
            bracket_depth: vec![3, 5, 6, 2, 4],
        }
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
    pub colors: ColorsConfig,
}

/// Environment variables the program reads (documented in docs/CUSTOMIZATION.md).
pub const ENV_VARS: &[&str] = &["KURO_CONFIG_DIR"];

pub const CLIPBOARD_BACKENDS: &[&str] = &["auto", "wl-clipboard", "xclip", "xsel", "osc52"];
pub const KITTY_MODES: &[&str] = &["auto", "on", "off"];
pub const INDENT_STYLES: &[&str] = &["tabs", "spaces"];
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
            colors: section(&table, "colors", &mut warnings),
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
        if !INDENT_STYLES.contains(&self.editor.indent_style.as_str()) {
            warnings
                .push(format!("editor.indent_style = {:?} is not one of {:?}; using \"tabs\"", self.editor.indent_style, INDENT_STYLES));
            self.editor.indent_style = "tabs".into();
        }
        if !KITTY_MODES.contains(&self.ui.kitty_keyboard.as_str()) {
            warnings.push(format!("ui.kitty_keyboard = {:?} is not one of {:?}; using \"auto\"", self.ui.kitty_keyboard, KITTY_MODES));
            self.ui.kitty_keyboard = "auto".into();
        }
        for (name, val, default) in [
            ("sql.run_fallback_key", &mut self.sql.run_fallback_key, "alt+enter"),
            ("sql.run_line_key", &mut self.sql.run_line_key, "alt+l"),
        ] {
            if crate::keyspec::parse(val).is_none() {
                warnings.push(format!("{name} = {val:?} is not a key like \"alt+enter\" or \"f9\"; using {default:?}"));
                *val = default.to_string();
            }
        }
        if !RESULT_VIEWS.contains(&self.sql.result_view.as_str()) {
            warnings.push(format!("sql.result_view = {:?} is not one of {:?}; using \"split\"", self.sql.result_view, RESULT_VIEWS));
            self.sql.result_view = "split".into();
        }
        let dc = ColorsConfig::default();
        macro_rules! slot {
            ($key:ident) => {
                if !(-1..=15).contains(&self.colors.$key) {
                    warnings.push(format!(
                        "colors.{} = {} is not an ANSI slot (0-15, or -1 for the default foreground); using {}",
                        stringify!($key),
                        self.colors.$key,
                        dc.$key
                    ));
                    self.colors.$key = dc.$key;
                }
            };
        }
        slot!(keyword);
        slot!(builtin);
        slot!(literal);
        slot!(string);
        slot!(number);
        slot!(operator);
        slot!(comment);
        slot!(key);
        slot!(bracket_error);
        if self.colors.bracket_depth.is_empty() || self.colors.bracket_depth.iter().any(|c| !(0..=15).contains(c)) {
            warnings.push(format!("colors.bracket_depth must be a non-empty list of ANSI slots 0-15; using {:?}", dc.bracket_depth));
            self.colors.bracket_depth = dc.bracket_depth.clone();
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
        at_least_one!(editor.indent_size);
        at_least_one!(editor.suggest_min_chars);
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
        assert_eq!((c.editor.indent_style.as_str(), c.editor.tab_width, c.editor.indent_size), ("tabs", 8, 8), "hard tabs, width 8");
        assert!(c.editor.modal);
        assert!(!c.editor.relative_numbers, "line numbers are absolute by default; relative is opt-in");
        assert_eq!(c.sql.table_max_width, 120);
        assert_eq!(c.sql.table_max_rows, 25);
    }

    #[test]
    fn overrides_apply() {
        let (c, w) = Config::from_toml(
            "[editor]\nmodal = false\ntab_width = 2\nindent_style = \"spaces\"\nindent_size = 2\n[sql]\ntable_max_rows = 10\n",
        );
        assert!(!c.editor.modal);
        assert_eq!(c.editor.tab_width, 2);
        assert_eq!((c.editor.indent_style.as_str(), c.editor.indent_size), ("spaces", 2));
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
        let (c, w) = Config::from_toml(
            "[clipboard]\nbackend = \"pigeon\"\n[editor]\nindent_style = \"mixed\"\n[sql]\nresult_view = \"x\"\ntable_max_rows = 0\n",
        );
        assert_eq!(c.clipboard.backend, "auto");
        assert_eq!(c.sql.result_view, "split");
        assert_eq!(c.sql.table_max_rows, 25);
        assert_eq!(w.len(), 4, "{w:?}");
        assert_eq!(c.editor.indent_style, "tabs");
    }

    #[test]
    fn color_roles_validate_ansi_slots() {
        let (c, w) = Config::from_toml("[colors]\nkeyword = 12\nstring = 99\ncomment = -1\nbracket_depth = [1, 2]\n");
        assert_eq!((c.colors.keyword, c.colors.comment, c.colors.bracket_depth.clone()), (12, -1, vec![1, 2]));
        assert_eq!(c.colors.string, 2, "out-of-range slot falls back to the default");
        assert_eq!(w.len(), 1, "{w:?}");
        let (c, w) = Config::from_toml("[colors]\nbracket_depth = []\n");
        assert_eq!(c.colors.bracket_depth, ColorsConfig::default().bracket_depth);
        assert_eq!(w.len(), 1);
        let (_, w) = Config::from_toml("[colors]\nbracket_depth = [3, 16]\n");
        assert_eq!(w.len(), 1);
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
