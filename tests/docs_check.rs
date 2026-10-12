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

const CUSTOMIZATION: &str = include_str!("../docs/CUSTOMIZATION.md");

/// The "Languages" table row for a mode, if any.
fn language_row(name: &str) -> Option<&'static str> {
    CUSTOMIZATION.lines().find(|l| l.starts_with(&format!("| {name} |")))
}

#[test]
fn every_language_mode_is_documented_with_its_extensions_and_aliases() {
    for spec in kuro::syntax::LANGS {
        let row =
            language_row(spec.name).unwrap_or_else(|| panic!("`{}` has no row in the Languages table of docs/CUSTOMIZATION.md", spec.name));
        for ext in spec.extensions {
            assert!(row.contains(&format!(".{ext}")), "`.{ext}` of {} is not in its row: {row}", spec.name);
        }
        for f in spec.filenames {
            assert!(row.contains(f), "file name {f} of {} is not in its row: {row}", spec.name);
        }
    }
}

#[test]
fn the_languages_table_lists_no_mode_that_does_not_exist() {
    let names: Vec<&str> = std::iter::once("Plain Text").chain(kuro::syntax::LANGS.iter().map(|l| l.name)).collect();
    let in_table = CUSTOMIZATION
        .lines()
        .skip_while(|l| !l.starts_with("## Languages"))
        .take_while(|l| !l.starts_with("### How HTML"))
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Mode") && !l.starts_with("|---"))
        .map(|l| l.trim_start_matches("| ").split(" |").next().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(in_table.len(), names.len(), "{in_table:?}");
    for n in &in_table {
        assert!(names.contains(&n.as_str()), "the table lists `{n}` which is not a mode");
    }
}

#[test]
fn every_documented_alias_resolves_to_its_row() {
    for spec in std::iter::once(&kuro::syntax::PLAIN).chain(kuro::syntax::LANGS.iter().copied()) {
        let row = language_row(spec.name).unwrap();
        let aliases = row.split('|').nth(2).unwrap();
        for a in aliases.split(',').map(|a| a.trim().trim_matches('`')).filter(|a| !a.is_empty()) {
            assert_eq!(kuro::syntax::by_name(a).map(|l| l.name), Some(spec.name), "alias `{a}` of {}", spec.name);
        }
    }
}

const EXTENSIONS: &str = include_str!("../docs/EXTENSIONS.md");

/// The field names serde accepts for a `deny_unknown_fields` struct, taken from the error it gives for an unknown key.
fn fields_of<T: serde::de::DeserializeOwned>() -> Vec<String> {
    let err = toml::from_str::<T>("zzz_unknown_key = 1").err().expect("an unknown key is an error").to_string();
    let after = err.split("expected").nth(1).unwrap_or("");
    after.split('`').skip(1).step_by(2).map(String::from).collect()
}

#[test]
fn every_manifest_field_is_documented() {
    use kuro::ext::manifest::{Declares, LanguageDef, Manifest, NamesFrom, Rule};
    for (what, fields) in [
        ("manifest", fields_of::<Manifest>()),
        ("[[language]]", fields_of::<LanguageDef>()),
        ("[[language.rule]]", fields_of::<Rule>()),
        ("[[language.declares]]", fields_of::<Declares>()),
        ("[[language.names_from]]", fields_of::<NamesFrom>()),
    ] {
        assert!(fields.len() >= 3, "{what}: could not read the field names ({fields:?})");
        for f in fields {
            let shown = EXTENSIONS.contains(&format!("{f} ="))
                || EXTENSIONS.contains(&format!("[[{f}]]"))
                || EXTENSIONS.contains(&format!(".{f}]"));
            assert!(shown, "the {what} field `{f}` is not shown in docs/EXTENSIONS.md");
        }
    }
}

#[test]
fn the_extension_commands_readers_and_roles_are_documented() {
    for cmd in ["kuro ext install", "kuro ext remove", "kuro ext list", "--force"] {
        assert!(EXTENSIONS.contains(cmd), "{cmd} is not documented");
    }
    for p in kuro::ext::manifest::PARSERS {
        assert!(EXTENSIONS.contains(p), "reader {p} is not documented");
    }
    for r in kuro::ext::manifest::CORE_ROLES {
        assert!(EXTENSIONS.contains(&format!("`{r}`")), "core role {r} is not documented");
    }
}

#[test]
fn the_ksystem_readme_names_every_role_with_its_default_slot() {
    let readme = include_str!("../extensions/ksystem/README.md");
    let m = kuro::ext::manifest::parse(include_str!("../extensions/ksystem/kuro-ext.toml"), env!("CARGO_PKG_VERSION")).unwrap();
    for (role, slot) in &m.language[0].roles {
        let row = readme
            .lines()
            .find(|l| l.starts_with(&format!("| `{role}` |")))
            .unwrap_or_else(|| panic!("role {role} is not in the README table"));
        assert!(row.contains(&format!("| {slot} (")), "the README row of {role} does not say slot {slot}: {row}");
    }
}

const HARD_UNDO: &str = include_str!("../docs/HARD-UNDO.md");

#[test]
fn hard_undo_docs_name_the_commands_and_every_history_option() {
    for cmd in [":hardundo", ":hardundo all", "kuro history list", "kuro history show", "kuro history delete", "kuro history clear"] {
        assert!(HARD_UNDO.contains(cmd), "{cmd} is not in docs/HARD-UNDO.md");
    }
    for opt in ["history.warn_mib", "history.exclude", "history.max_file_mib", "history.enabled"] {
        assert!(HARD_UNDO.contains(opt), "{opt} is not in docs/HARD-UNDO.md");
    }
    for pat in kuro::history::DEFAULT_EXCLUDE {
        assert!(HARD_UNDO.contains(pat), "the default exclude pattern {pat} is not in docs/HARD-UNDO.md");
    }
    // the usage text of the command line and the docs agree on the sub-commands
    for sub in ["list", "show", "delete", "clear"] {
        assert!(kuro::history::cli::USAGE.contains(&format!("kuro history {sub}")), "{sub}");
    }
}
