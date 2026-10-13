//! The ksystem language extension (kept in the extension registry, `kuro ext install ksystem`; a copy of its manifest is in `tests/fixtures/ksystem-ext/`), tested against small invented fixtures that only imitate the structure of
//! ksystem files (nothing from a real system is included).

mod common;
use common::snap::assert_snapshot;
use common::ui::*;
use kuro::app::App;
use kuro::config::{Config, ExtConfig};
use kuro::ext::lang::{self, Role};
use kuro::ext::manifest;
use kuro::ext::names;
use kuro::syntax::{self, Kind, LangSpec};
use std::collections::HashMap;
use std::path::Path;

const MANIFEST: &str = include_str!("fixtures/ksystem-ext/kuro-ext.toml");
const SCREEN: &str = include_str!("fixtures/ksystem/screen.sct");

fn ksystem() -> &'static LangSpec {
    let m = manifest::parse(MANIFEST, env!("CARGO_PKG_VERSION")).unwrap_or_else(|e| panic!("{e:?}"));
    lang::build(&m)[0]
}

fn headers_dir() -> String {
    format!("{}/tests/fixtures/ksystem/headers/*.h", env!("CARGO_MANIFEST_DIR"))
}

fn with_names(spec: &'static LangSpec) -> &'static LangSpec {
    let e = spec.ext.unwrap();
    let mut into = HashMap::new();
    let mut warnings = vec![];
    for src in &e.names_from {
        names::load(&[headers_dir()], &src.parser, src.role, &mut into, &mut warnings);
    }
    assert!(warnings.is_empty(), "{warnings:?}");
    lang::set_external_names(spec, into);
    spec
}

fn lines(src: &str) -> Vec<String> {
    src.split('\n').map(str::to_string).collect()
}

/// The kind of the first occurrence of `needle` on the first line that contains it at or after `from_line`.
fn kind(spec: &LangSpec, src: &str, from_line: usize, needle: &str) -> Kind {
    let ls = lines(src);
    let ks = syntax::highlight(spec, &ls);
    for (i, l) in ls.iter().enumerate().skip(from_line) {
        if let Some(at) = l.find(needle) {
            return ks[i][l[..at].chars().count()];
        }
    }
    panic!("{needle:?} not found");
}

fn own(spec: &LangSpec, role: &str) -> Kind {
    Kind::Ext(spec.ext.unwrap().role_index(role).unwrap())
}

fn line_of(src: &str, needle: &str) -> usize {
    lines(src).iter().position(|l| l.contains(needle)).unwrap()
}

#[test]
fn the_manifest_is_valid_and_declares_four_roles_with_distinct_default_slots() {
    let m = manifest::parse(MANIFEST, env!("CARGO_PKG_VERSION")).unwrap();
    assert_eq!(m.name, "ksystem");
    let l = &m.language[0];
    assert_eq!((l.name.as_str(), l.aliases.clone()), ("ksystem", vec!["ks".to_string()]));
    let slots: Vec<i64> = ["local", "calc", "field", "index"].iter().map(|r| l.roles[*r]).collect();
    let mut distinct = slots.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 4, "four different default colours: {slots:?}");
    assert!(slots.iter().all(|s| (0..=15).contains(s)));
}

mod detection {
    use super::*;

    fn detect(ext: &[&'static LangSpec], path: &str, first: &str) -> &'static str {
        syntax::detect_in(ext, Some(Path::new(path)), first).name
    }

    #[test]
    fn screens_are_found_by_file_name_variable_tables_by_extension() {
        let ks = [ksystem()];
        for p in ["sct180", "/srv/x/sct1", "/a/tmp_1008demografi", "tmp_x", "/a/b/gv.var", "t.VAR"] {
            assert_eq!(detect(&ks, p, ""), "ksystem", "{p}");
        }
        // other names are not claimed
        for p in ["sct", "sctx", "sct12.txt", "tmp", "tmp_a.b", "tmpfile", "notes"] {
            assert_ne!(detect(&ks, p, "plain text"), "ksystem", "{p}");
        }
    }

    #[test]
    fn html_and_xml_and_other_known_types_stay_with_their_own_modes() {
        let ks = [ksystem()];
        assert_eq!(detect(&ks, "sct135.htm", "<!DOCTYPE HTML>"), "HTML");
        assert_eq!(detect(&ks, "page.html", "SCREEN 1 QUERY_TYPE x"), "HTML", "a content marker never takes over a known file type");
        assert_eq!(detect(&ks, "rep.rptdesign", ""), "Plain Text");
        assert_eq!(detect(&ks, "bhs.xml", "<?xml version=\"1.0\"?>"), "XML");
        assert_eq!(detect(&ks, "x.c", "/****************************************/"), "C", "a C banner is not ksystem");
        assert_eq!(detect(&ks, "run.sh", "#!/bin/sh"), "Shell");
    }

    #[test]
    fn files_with_no_usable_name_are_recognised_by_how_they_start() {
        let ks = [ksystem()];
        for first in [
            "SCREEN 12 QUERY_TYPE s.X",
            "IMAGE(akthead) ",
            "MAP(m1)",
            "FIELD(master)",
            "CALCS()",
            "DBCALCS(d1)",
            "/*****************************/",
        ] {
            assert_eq!(detect(&ks, "scan/sakt", first), "ksystem", "{first}");
        }
        assert_eq!(syntax::detect_in(&ks, None, "SCREEN 1 QUERY_TYPE x").name, "ksystem", "a new buffer too");
        assert_eq!(detect(&ks, "scan/other", "hello"), "Plain Text");
        assert_eq!(detect(&ks, "README", "IMAGES of cats"), "Plain Text", "IMAGE( needs the parenthesis");
    }

    #[test]
    fn without_the_extension_installed_nothing_is_ksystem() {
        assert_eq!(detect(&[], "sct180", "SCREEN 1"), "Plain Text");
        assert!(syntax::by_name_in(&[], "ksystem").is_none());
    }

    #[test]
    fn setlang_names_and_alias_resolve() {
        let ks = [ksystem()];
        for n in ["ksystem", "KSystem", "ks", "KS"] {
            assert_eq!(syntax::by_name_in(&ks, n).map(|l| l.name), Some("ksystem"), "{n}");
        }
        assert_eq!(syntax::by_name_in(&ks, "python").map(|l| l.name), Some("Python"), "core modes still resolve");
        assert!(syntax::all_modes_in(&ks).iter().any(|l| l.name == "ksystem"));
    }
}

#[test]
fn sections_actions_attributes_types_and_the_families() {
    let k = ksystem();
    for (needle, want) in [
        ("SCREEN", Kind::Keyword),
        ("QUERY_TYPE", Kind::Keyword),
        ("SCR_FILE", Kind::Keyword),
        ("SCR_JOIN_FILE1", Kind::Keyword),
        ("PGM_FIELDS", Kind::Keyword),
        ("LOCAL", Kind::Keyword),
        ("STRING", Kind::Builtin),
        ("NUMERIC", Kind::Builtin),
        ("MAP(", Kind::Keyword),
        ("REPLACE_IF", Kind::Keyword),
        ("DO_CALC", Kind::Keyword),
        ("DBCALCS(", Kind::Keyword),
        ("DBC_COMMAND", Kind::Keyword),
        ("DBC_TARGET", Kind::Keyword),
        ("IMAGE(", Kind::Keyword),
        ("ALLCAPS", Kind::Builtin),
        ("INPUTRI", Kind::Builtin),
        ("PROTECT", Kind::Builtin),
        ("AFTER_INPUT", Kind::Keyword),
        ("DO_LIB_IF", Kind::Keyword),
        ("INIT", Kind::Keyword),
        ("END_SCREEN", Kind::Keyword),
    ] {
        assert_eq!(kind(k, SCREEN, 0, needle), want, "{needle}");
    }
    assert_eq!(kind(k, SCREEN, line_of(SCREEN, "DBC_COMMAND"), "JOIN"), Kind::Builtin, "a DBC_COMMAND value");
    // the section header's whole word, not just its first letter
    let header = line_of(SCREEN, "MAP(mfill)");
    let ks = syntax::highlight(k, &lines(SCREEN));
    assert!(ks[header][..3].iter().all(|x| *x == Kind::Keyword));
    assert_eq!(ks[header][3], Kind::Bracket(0), "the parenthesis keeps its bracket colour");
}

#[test]
fn comments_are_only_slash_star_and_may_span_lines() {
    let k = ksystem();
    let first = line_of(SCREEN, "a comment that");
    let ks = syntax::highlight(k, &lines(SCREEN));
    let ls = lines(SCREEN);
    assert!(ks[0].iter().all(|x| *x == Kind::Comment), "the banner line");
    assert!(ks[first].iter().all(|x| *x == Kind::Comment));
    let second = first + 1;
    assert!(ks[second].iter().all(|x| *x == Kind::Comment), "the comment continues: {:?}", ls[second]);
    // a `#` line is NOT a comment, and neither is `//`
    assert_ne!(kind(k, SCREEN, 0, "#lname"), Kind::Comment);
    assert_ne!(kind(k, "x // not a comment\n", 0, "// not"), Kind::Comment);
    // code resumes after the closing */
    assert_eq!(kind(k, "/* c */ MAP(x)\n", 0, "MAP"), Kind::Keyword);
    // names inside a comment are not coloured as variables
    assert_eq!(kind(k, SCREEN, first, "cpick"), Kind::Comment);
}

#[test]
fn strings_may_span_lines_and_swallow_everything_inside() {
    let k = ksystem();
    let a = line_of(SCREEN, "a note");
    let ks = syntax::highlight(k, &lines(SCREEN));
    assert_eq!(kind(k, SCREEN, a, "\"a note"), Kind::String);
    let next = &ks[a + 1];
    let closing = lines(SCREEN)[a + 1].find(')').unwrap();
    assert!(next[..closing].iter().all(|x| *x == Kind::String), "the second line of the string, including the names in it");
    assert_ne!(next[closing], Kind::String, "code resumes after the closing quote");
    // names in the string are not coloured as variables (cready, lcount)
    assert_eq!(kind(k, SCREEN, a + 1, "cready"), Kind::String);
}

#[test]
fn operators_and_the_address_and_template_forms() {
    let k = ksystem();
    for op in ["==", "&&", "||", "!=", ">=", "<="] {
        assert_eq!(kind(k, SCREEN, 0, op), Kind::Operator, "{op}");
    }
    assert_eq!(kind(k, SCREEN, 0, "&ltotal"), Kind::Builtin, "passed by address");
    assert_eq!(kind(k, SCREEN, line_of(SCREEN, "INIT {"), "cready"), Kind::Literal, "template substitution (the braces stay brackets)");
    assert_eq!(kind(k, SCREEN, 0, "21"), Kind::Number);
    let h = line_of(SCREEN, "AFTER_INPUT");
    let ks = syntax::highlight(k, &lines(SCREEN));
    assert!(ks[h].iter().any(|x| matches!(x, Kind::Bracket(_))), "( ) get bracket colours");
}

mod variables {
    use super::*;

    #[test]
    fn locals_declared_under_pgm_fields_get_the_local_colour_everywhere() {
        let k = ksystem();
        let local = own(k, "local");
        for needle in ["lcount\t\tSTRING", "lname\t\tSTRING", "ltotal\t\tNUMERIC"] {
            let at = needle.split('\t').next().unwrap();
            assert_eq!(kind(k, SCREEN, 0, needle).clone(), local, "declaration of {at}");
        }
        // used in a condition, in a MAP column, in a hook
        let cond = line_of(SCREEN, "cready\t\tlcount");
        let ks = syntax::highlight(k, &lines(SCREEN));
        let col = lines(SCREEN)[cond].find("lcount").unwrap();
        assert_eq!(ks[cond][col], local, "in a CALCS condition");
        assert_eq!(kind(k, SCREEN, line_of(SCREEN, "dmoname\t\tlname"), "lname"), local, "in a MAP row");
        assert_eq!(kind(k, SCREEN, line_of(SCREEN, "DEFAULTS"), "lname"), local, "in DEFAULTS");
        // the whole name is coloured, not its first letter
        let decl = line_of(SCREEN, "lcount\t\tSTRING");
        assert!(ks[decl][..6].iter().all(|x| *x == local));
    }

    #[test]
    fn a_name_that_merely_starts_with_l_is_not_a_local() {
        let k = ksystem();
        assert_eq!(kind(k, SCREEN, 0, "lother"), Kind::Plain, "`lother` is never declared");
        assert_eq!(kind(k, "PGM_FIELDS\nlx STRING 1\nMAP(m)\nlx lother REPLACE\n", 0, "lother"), Kind::Plain);
        // declared names only count in PGM_FIELDS, not in other sections
        assert_eq!(kind(k, "MAP(m)\nlzzz STRING 1\nlzzz\n", 0, "lzzz"), Kind::Plain);
    }

    #[test]
    fn calcs_declared_under_calcs_get_their_own_colour() {
        let k = ksystem();
        let calc = own(k, "calc");
        assert_ne!(calc, own(k, "local"), "locals and calcs differ");
        let decl = line_of(SCREEN, "cready\t\tlcount");
        let ks = syntax::highlight(k, &lines(SCREEN));
        assert!(ks[decl][..6].iter().all(|x| *x == calc), "the declaration");
        assert_eq!(kind(k, SCREEN, line_of(SCREEN, "REPLACE_IF"), "cready"), calc, "used after REPLACE_IF");
        assert_eq!(kind(k, SCREEN, line_of(SCREEN, "AFTER_INPUT"), "cready"), calc, "used in a hook");
        assert_eq!(kind(k, SCREEN, line_of(SCREEN, "INIT {"), "cready"), Kind::Literal, "inside the braces it is the template form");
        assert_eq!(kind(k, SCREEN, 0, "cpick"), calc);
    }

    #[test]
    fn a_calc_may_hold_anything_and_is_declared_by_its_name_only() {
        let k = ksystem();
        let src = "CALCS()\ncnum\t5\ncstr\t\"text\"\ncanother\tcnum==5\n";
        for n in ["cnum", "cstr", "canother"] {
            assert_eq!(kind(k, src, 0, n), own(k, "calc"), "{n}");
        }
        assert_eq!(kind(k, src, 3, "cnum"), own(k, "calc"), "and a later use");
    }

    #[test]
    fn declarations_of_one_file_do_not_leak_into_another() {
        let k = ksystem();
        let a = "PGM_FIELDS\nlonly STRING 1\n";
        assert_eq!(kind(k, a, 0, "lonly"), own(k, "local"));
        assert_eq!(kind(k, "MAP(m)\nlonly x REPLACE\n", 0, "lonly"), Kind::Plain, "another file");
    }
}

mod file_fields_and_indexes {
    use super::*;

    #[test]
    fn fields_and_indexes_come_from_the_headers_the_user_lists() {
        let k = with_names(ksystem());
        let (field, index) = (own(k, "field"), own(k, "index"));
        assert_ne!(field, index);
        for f in ["dmokode", "dmoname", "dmoamount", "dmostatus", "dmoprm"] {
            assert_eq!(kind(k, &format!("MAP(m)\n{f} x REPLACE\n"), 1, f), field, "{f}");
        }
        for i in ["dmoidx", "dmodat"] {
            assert_eq!(kind(k, &format!("MAP(m)\n{i} x REPLACE\n"), 1, i), index, "{i}");
        }
        // everywhere in the fixture
        assert_eq!(kind(k, SCREEN, line_of(SCREEN, "SCR_LOGFLD"), "dmokode"), field);
        assert_eq!(kind(k, SCREEN, line_of(SCREEN, "SCR_JOIN_FILE1"), "dmoidx"), index);
        assert_eq!(kind(k, SCREEN, line_of(SCREEN, "DBC_TARGET"), "dmokode"), field);
        assert_eq!(kind(k, SCREEN, line_of(SCREEN, "cready\t\tlcount"), "dmostatus"), field, "in a condition");
    }

    #[test]
    fn without_a_headers_setting_nothing_is_coloured_as_a_field() {
        let k = ksystem();
        assert_eq!(kind(k, "MAP(m)\ndmokode x REPLACE\n", 1, "dmokode"), Kind::Plain);
    }

    #[test]
    fn fields_are_coloured_wherever_they_appear_but_not_inside_strings_and_comments() {
        let k = with_names(ksystem());
        assert_eq!(kind(k, "PGM_FIELDS\ndmokode STRING 1\n", 1, "dmokode"), own(k, "field"), "a name is a field wherever it is written");
        let src = "MAP(m)\nx \"dmokode in a string\"\n/* dmokode in a comment */\n";
        assert_eq!(kind(k, src, 1, "dmokode"), Kind::String);
        assert_eq!(kind(k, src, 2, "dmokode"), Kind::Comment);
    }

    #[test]
    fn warnings_for_a_pattern_that_matches_nothing_and_for_wildcards_in_folders() {
        let mut into = HashMap::new();
        let mut w = vec![];
        names::load(&["/nonexistent-dir-kuro/*.h".to_string()], "c-struct-members", Role::Core(Kind::Plain), &mut into, &mut w);
        assert!(w.iter().any(|x| x.contains("cannot read")), "{w:?}");
        let mut w = vec![];
        names::load(&["/a/*/x.h".to_string()], "c-struct-members", Role::Core(Kind::Plain), &mut into, &mut w);
        assert!(w[0].contains("only allowed in the file name"));
        let mut w = vec![];
        names::load(
            &[format!("{}/tests/fixtures/ksystem/headers/zzz*.h", env!("CARGO_MANIFEST_DIR"))],
            "c-struct-members",
            Role::Core(Kind::Plain),
            &mut into,
            &mut w,
        );
        assert!(w[0].contains("matches no files"), "{w:?}");
        assert!(into.is_empty());
    }
}

#[test]
fn image_blocks_colour_their_labels_and_input_slots_only_inside_image() {
    let k = ksystem();
    assert_eq!(kind(k, SCREEN, 0, "i.Code"), Kind::Key);
    assert_eq!(kind(k, SCREEN, 0, "t.Name"), Kind::Key);
    assert_eq!(kind(k, SCREEN, 0, "________"), Kind::Literal);
    // the same text outside an IMAGE block is not a label
    assert_ne!(kind(k, "MAP(m)\ni.Code x\n", 1, "i.Code"), Kind::Key);
    // and the block ends at the next section
    assert_ne!(kind(k, "IMAGE(a)\n| i.A |\nFIELD(a)\ni.B\n", 3, "i.B"), Kind::Key);
}

fn app_with_ksystem(headers: bool) -> App {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    if headers {
        let mut e = ExtConfig::default();
        e.settings.insert("headers".into(), vec![headers_dir()]);
        cfg.ext.insert("ksystem".into(), e);
    }
    let (mut app, _) = app_cfg(SCREEN, cfg);
    app.set_ext_langs(vec![ksystem()]);
    app
}

#[test]
fn the_user_can_map_each_role_and_the_defaults_are_palette_slots() {
    use ratatui::style::Color;
    let mut app = app_with_ksystem(false);
    app.set_lang(syntax::by_name_in(&app.ext_langs, "ksystem"));
    let roles: Vec<String> = app.lang.ext.unwrap().roles.iter().map(|r| r.name.clone()).collect();
    assert_eq!(roles.len(), 4);
    let idx = |n: &str| roles.iter().position(|r| r == n).unwrap();
    // defaults: bright cyan / yellow / green / magenta, all ANSI
    assert_eq!(app.ext_styles[idx("local")].fg, Some(Color::LightCyan));
    assert_eq!(app.ext_styles[idx("calc")].fg, Some(Color::LightYellow));
    assert_eq!(app.ext_styles[idx("field")].fg, Some(Color::LightGreen));
    assert_eq!(app.ext_styles[idx("index")].fg, Some(Color::LightMagenta));
    // [ext.ksystem.colors] overrides one role and -1 means the terminal's default text colour
    let mut cfg = Config::default();
    let mut e = ExtConfig::default();
    e.colors.insert("local".into(), 6);
    e.colors.insert("index".into(), -1);
    cfg.ext.insert("ksystem".into(), e);
    let (mut app, _) = app_cfg(SCREEN, cfg);
    app.set_ext_langs(vec![ksystem()]);
    app.set_lang(syntax::by_name_in(&app.ext_langs, "ks"));
    assert_eq!(app.ext_styles[idx("local")].fg, Some(Color::Cyan));
    assert_eq!(app.ext_styles[idx("index")].fg, None, "the default foreground");
    assert_eq!(app.ext_styles[idx("calc")].fg, Some(Color::LightYellow), "unmapped roles keep their default");
}

#[test]
fn config_slots_outside_the_palette_are_reported() {
    let (cfg, warnings) = Config::from_toml("[ext.ksystem.colors]\nlocal = 99\ncalc = 3\n[ext.ksystem]\nheaders = [\"/x/*.h\"]\n");
    assert!(warnings.iter().any(|w| w.contains("ext.ksystem.colors.local")), "{warnings:?}");
    assert!(!warnings.iter().any(|w| w.contains("unknown config section")), "{warnings:?}");
    assert_eq!(cfg.ext["ksystem"].settings["headers"], vec!["/x/*.h".to_string()]);
}

#[test]
fn setlang_picks_ksystem_recolours_and_the_picker_lists_it() {
    let mut app = app_with_ksystem(false);
    app.exec_command("setlang ksystem");
    assert_eq!(app.lang.name, "ksystem");
    app.exec_command("setlang auto");
    app.exec_command("setlang");
    let s = screen(&mut app).join("\n");
    assert!(
        s.contains("ksystem") || {
            press(&mut app, "up");
            screen(&mut app).join("\n").contains("ksystem")
        },
        "the picker lists the extension's language:\n{s}"
    );
}

#[test]
fn a_file_named_like_a_screen_opens_as_ksystem_and_its_headers_warning_names_the_extension() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("sct4711");
    std::fs::write(&f, "SCREEN 1 QUERY_TYPE x\n").unwrap();
    let mut cfg = Config::default();
    let mut e = ExtConfig::default();
    e.settings.insert("headers".into(), vec![format!("{}/none*.h", dir.path().display())]);
    cfg.ext.insert("ksystem".into(), e);
    let (mut app, _) = app_cfg("", cfg);
    app.set_ext_langs(vec![ksystem()]);
    app.open_file(&f);
    assert_eq!(app.lang.name, "ksystem");
    assert!(app.warnings.iter().any(|w| w.contains("extension ksystem") && w.contains("matches no files")), "{:?}", app.warnings);
}

#[test]
fn the_rendered_screen_uses_only_palette_colours_and_no_background() {
    use ratatui::style::Color;
    let mut app = app_with_ksystem(true);
    app.set_lang(syntax::by_name_in(&app.ext_langs, "ksystem"));
    let buf = render_size(&mut app, 100, 50);
    for y in 0..50 {
        for x in 0..100 {
            let c = &buf[(x, y)];
            assert_eq!(c.bg, Color::Reset, "background at ({x},{y})");
            assert!(!matches!(c.fg, Color::Rgb(..) | Color::Indexed(_)), "({x},{y}) {:?}", c.fg);
        }
    }
}

#[test]
fn snapshot_of_a_ksystem_screen_with_headers_loaded() {
    let mut app = app_with_ksystem(true);
    app.set_lang(syntax::by_name_in(&app.ext_langs, "ksystem"));
    let buf = render_size(&mut app, 100, 48);
    assert_snapshot("ext_ksystem_screen", &buf);
}
