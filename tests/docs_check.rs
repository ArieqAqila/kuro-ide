//! Docs check: every option in code is documented in docs/CUSTOMIZATION.md, and vice versa,
//! with matching defaults. The checker itself is tested against deliberately broken docs.

use kuro::config::{ENV_VARS, option_defaults};
use kuro::sql::profile::profile_fields;
use std::collections::{BTreeMap, BTreeSet};

const DOCS: &str = include_str!("../docs/CUSTOMIZATION.md");

/// `### \`a.b\`` headings -> (key, Default value text if a `Default` bullet follows).
fn documented(docs: &str) -> BTreeMap<String, Option<String>> {
    let mut out = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in docs.lines() {
        if let Some(h) = line.strip_prefix("### `") {
            let key = h.trim_end_matches('`').to_string();
            out.insert(key.clone(), None);
            current = Some(key);
        } else if let (Some(k), Some(d)) = (&current, line.strip_prefix("- **Default:** `")) {
            let val = d.split('`').next().unwrap().to_string();
            out.insert(k.clone(), Some(val));
        } else if line.starts_with('#') {
            current = None;
        }
    }
    out
}

/// Compare documented option keys with the code's; returns human-readable problems.
fn check(docs: &str, code: &BTreeMap<String, String>, prefix_filter: impl Fn(&str) -> bool) -> Vec<String> {
    let doc = documented(docs);
    let mut problems = vec![];
    for (k, default) in code {
        match doc.get(k) {
            None => problems.push(format!("option `{k}` exists in code but is missing from docs/CUSTOMIZATION.md")),
            Some(None) => problems.push(format!("option `{k}` is documented without a `- **Default:**` line")),
            Some(Some(d)) if d != default => problems.push(format!("option `{k}`: docs say default {d}, code says {default}")),
            _ => {}
        }
    }
    for k in doc.keys().filter(|k| prefix_filter(k)) {
        if !code.contains_key(k) {
            problems.push(format!("`{k}` is documented but no such option exists in code"));
        }
    }
    problems
}

fn code_options() -> BTreeMap<String, String> {
    option_defaults().into_iter().map(|(k, v)| (k, v.to_string())).collect()
}

fn code_profile_fields() -> BTreeMap<String, String> {
    // profile fields have no single default; only presence is checked
    profile_fields().into_iter().map(|f| (format!("profile.{f}"), String::new())).collect()
}

fn is_config_key(k: &str) -> bool {
    !k.starts_with("profile.")
}

#[test]
fn config_options_match_the_docs() {
    // profile fields are checked separately: strip the Default comparison for them
    let problems = check(DOCS, &code_options(), is_config_key);
    assert!(problems.is_empty(), "docs/CUSTOMIZATION.md is out of sync:\n  {}", problems.join("\n  "));
}

#[test]
fn profile_fields_match_the_docs() {
    let doc = documented(DOCS);
    let code: BTreeSet<String> = code_profile_fields().into_keys().collect();
    let docs: BTreeSet<String> = doc.keys().filter(|k| k.starts_with("profile.")).cloned().collect();
    assert_eq!(docs, code, "profile fields in docs vs code");
}

#[test]
fn environment_variables_are_documented() {
    for v in ENV_VARS {
        assert!(DOCS.contains(&format!("`{v}`")), "env var {v} is read by the program but not documented");
    }
}

#[test]
fn committed_default_config_documents_the_same_keys() {
    let text = include_str!("../config/default/config.toml");
    let table: toml::Table = text.parse().unwrap();
    let mut keys = BTreeSet::new();
    for (s, v) in &table {
        for k in v.as_table().unwrap().keys() {
            keys.insert(format!("{s}.{k}"));
        }
    }
    let code: BTreeSet<String> = option_defaults().into_keys().collect();
    assert_eq!(keys, code, "config/default/config.toml must list every option");
}

// ---- the checker must actually catch drift (otherwise the checks above prove nothing) ----

#[test]
fn checker_detects_option_missing_from_docs() {
    let broken = DOCS.replace("### `editor.tab_width`", "### `editor.tab_widthh`");
    let problems = check(&broken, &code_options(), is_config_key);
    assert!(problems.iter().any(|p| p.contains("`editor.tab_width` exists in code but is missing")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("`editor.tab_widthh` is documented but no such option")), "{problems:?}");
}

#[test]
fn checker_detects_a_documented_option_that_does_not_exist() {
    let broken = format!("{DOCS}\n### `editor.ghost`\n- **Default:** `1`\n");
    let problems = check(&broken, &code_options(), is_config_key);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("editor.ghost"));
}

#[test]
fn checker_detects_a_wrong_default() {
    let broken = DOCS.replace("- **Default:** `120`", "- **Default:** `121`");
    let problems = check(&broken, &code_options(), is_config_key);
    assert!(problems.iter().any(|p| p.contains("sql.table_max_width") && p.contains("121")), "{problems:?}");
}

#[test]
fn checker_detects_a_missing_default_line() {
    let broken = DOCS.replace("### `find.regex`\n- **Type:** boolean\n- **Default:** `false`", "### `find.regex`\n- **Type:** boolean");
    let problems = check(&broken, &code_options(), is_config_key);
    assert!(problems.iter().any(|p| p.contains("find.regex") && p.contains("without")), "{problems:?}");
}

#[test]
fn checker_passes_on_good_docs_and_new_code_options_would_fail() {
    assert!(check(DOCS, &code_options(), is_config_key).is_empty());
    let mut code = code_options();
    code.insert("editor.brand_new".into(), "true".into());
    let problems = check(DOCS, &code, is_config_key);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("editor.brand_new"));
}
