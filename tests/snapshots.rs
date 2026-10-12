//! Snapshot tests: whole rendered screens at fixed terminal sizes, compared with committed files in
//! `tests/snapshots/`. See docs/TESTING.md. Update with `KURO_UPDATE_SNAPSHOTS=1 cargo test --test snapshots`.

mod common;
use common::snap::assert_snapshot;
use common::sqlview::*;
use common::ui::*;
use kuro::app::App;
use kuro::config::Config;
use kuro::sql::profile::Engine;
use kuro::sql::{Canceller, DbError, Driver, ResultSet, Session};
use kuro::syntax;
use kuro::table::Column;
use std::sync::Arc;

const W: u16 = 64;
const H: u16 = 16;

const GO: &str = "package main\n\nimport \"fmt\"\n\n// greet prints a greeting\nfunc greet(names []string) {\n\tfor i, n := range names {\n\t\tif i >= 0 && n != \"\" {\n\t\t\tfmt.Println(\"hello\", n, 0x1F, 2.5e-3)\n\t\t}\n\t}\n}\n";

fn shot(name: &str, app: &mut App) {
    let buf = render_size(app, W, H);
    assert_snapshot(name, &buf);
}

fn go_app() -> App {
    let (mut app, _) = app_with(GO);
    app.set_lang(Some(&syntax::GO));
    app
}

struct Db(Engine);
impl Driver for Db {
    fn execute(&mut self, _s: &str, _l: usize) -> Result<ResultSet, DbError> {
        Ok(ResultSet::default())
    }
    fn canceller(&self) -> Canceller {
        Arc::new(|| Ok(()))
    }
    fn label(&self) -> String {
        "db".into()
    }
    fn engine(&self) -> Engine {
        self.0
    }
}

#[test]
fn the_serializer_marks_text_style_and_colour_and_handles_wide_chars() {
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Style};
    let mut b = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 6, 1));
    b.set_string(0, 0, "a日b", Style::default().fg(Color::Red).add_modifier(Modifier::REVERSED));
    let s = common::snap::serialize(&b);
    assert!(s.contains("|a日b  |"), "{s}");
    assert!(s.contains("|RRRR..|"), "a wide char covers two style cells: {s}");
    assert!(s.contains("|rrrr..|"), "{s}");
}

#[test]
fn editor_with_go_syntax_colours() {
    shot("editor_go", &mut go_app());
}

#[test]
fn editor_with_relative_numbers_and_cursor_in_the_middle() {
    let mut cfg = Config::default();
    cfg.editor.relative_numbers = true;
    let (mut app, _) = app_cfg(GO, cfg);
    app.set_lang(Some(&syntax::GO));
    press(&mut app, "6 j");
    shot("editor_relative_numbers", &mut app);
}

#[test]
fn visual_selection_across_lines() {
    let mut app = go_app();
    press(&mut app, "j j j w v j j l l");
    shot("visual_selection", &mut app);
}

#[test]
fn find_bar_with_highlighted_matches() {
    let mut app = go_app();
    press(&mut app, "ctrl+f");
    typed(&mut app, "n");
    press(&mut app, "enter");
    shot("find_bar", &mut app);
}

#[test]
fn find_and_replace_bar() {
    let mut app = go_app();
    press(&mut app, "ctrl+h");
    typed(&mut app, "names");
    press(&mut app, "tab");
    typed(&mut app, "people");
    shot("find_replace_bar", &mut app);
}

#[test]
fn find_bar_with_options_and_no_results() {
    let mut app = go_app();
    press(&mut app, "ctrl+f alt+c alt+w");
    typed(&mut app, "Names");
    shot("find_bar_no_results_case_word", &mut app);
}

#[test]
fn find_bar_with_invalid_regex() {
    let mut app = go_app();
    press(&mut app, "ctrl+f alt+r");
    typed(&mut app, "(");
    shot("find_bar_bad_regex", &mut app);
}

#[test]
fn result_table_pane() {
    let (mut app, _) = app_with("select 1;");
    let mut rs = ResultSet {
        columns: vec![
            Column { name: "id".into(), type_name: "int4".into() },
            Column { name: "name".into(), type_name: "text".into() },
            Column { name: "note".into(), type_name: "text".into() },
        ],
        rows: vec![
            vec!["1".into(), "alpha".into(), "NULL".into()],
            vec!["2".into(), "日本語".into(), "a very long value that is cut with an ellipsis because it exceeds the width".into()],
            vec!["3".into(), "gamma".into(), "NULL".into()],
        ],
        total_rows: 3,
        ..Default::default()
    };
    rs.nulls.insert((0, 2));
    rs.nulls.insert((2, 2));
    show(&mut app, rs);
    shot("result_table", &mut app);
    press(&mut app, "f4 l");
    shot("result_table_focused_column_2", &mut app);
}

#[test]
fn result_table_scrolled_with_pinned_header() {
    let (mut app, _) = app_with("select 1;");
    show_rows(&mut app, 60, 5);
    press(&mut app, "f4 pagedown j j");
    shot("result_table_scrolled", &mut app);
}

#[test]
fn result_error_panes_for_each_category() {
    for (cat, msg) in [
        ("syntax", "syntax error at or near \"SELEC\""),
        ("runtime", "division by zero"),
        ("execution", "relation \"nope\" does not exist"),
    ] {
        let (mut app, _) = app_with("SELEC 1;");
        show_error(&mut app, cat, msg, Some(1), Some(1));
        shot(&format!("error_{cat}"), &mut app);
    }
}

#[test]
fn results_in_a_tab() {
    let mut cfg = Config::default();
    cfg.sql.result_view = "tab".into();
    let (mut app, _) = app_cfg("select 1;", cfg);
    show_rows(&mut app, 4, 3);
    press(&mut app, "f4");
    shot("results_tab", &mut app);
}

#[test]
fn word_suggestion_popup() {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    let (mut app, _) = app_cfg("hello helmet help world\nanother here\n", cfg);
    app.buffer.cursor = app.buffer.len_chars();
    typed(&mut app, "he");
    press(&mut app, "down");
    shot("suggestions", &mut app);
}

#[test]
fn language_picker() {
    let (mut app, _) = app_with("x");
    press(&mut app, ":");
    typed(&mut app, "setlang");
    press(&mut app, "enter");
    press(&mut app, "down down");
    shot("language_picker", &mut app);
}

#[test]
fn snippet_picker_generic_and_per_driver() {
    let (mut app, _) = app_with("");
    press(&mut app, "alt+s");
    shot("snippets_generic", &mut app);
    let (mut app, _) = app_with("");
    app.sql.session = Some(Session::new(Box::new(Db(Engine::MariaDb))));
    press(&mut app, "alt+s");
    shot("snippets_mariadb", &mut app);
    let (mut app, _) = app_with("");
    app.sql.session = Some(Session::new(Box::new(Db(Engine::Postgres))));
    press(&mut app, "alt+s");
    typed(&mut app, "table");
    shot("snippets_postgres_filtered", &mut app);
}

#[test]
fn disconnected_sql_buffer() {
    let (mut app, _) = app_with("select * from users;\n");
    app.set_lang(Some(&syntax::SQL));
    shot("sql_disconnected", &mut app);
}

#[test]
fn bracket_depth_and_mismatch_map() {
    let (mut app, _) = app_with("f(a[b{c}], (d)]\n(unclosed\n");
    app.set_lang(Some(&syntax::GO));
    shot("brackets", &mut app);
}

#[test]
fn gutter_at_100_lines_and_scrolled_to_the_end() {
    let text: String = (1..=120).map(|i| format!("row {i}\n")).collect();
    let (mut app, _) = app_with(&text);
    press(&mut app, "G");
    shot("gutter_three_digits_end", &mut app);
}

#[test]
fn tabs_displayed_at_width_8_and_4() {
    let (mut app, _) = app_with("\tone\n\t\ttwo\nx\ty\tz\n");
    shot("tabs_width_8", &mut app);
    let mut cfg = Config::default();
    cfg.editor.tab_width = 4;
    let (mut app, _) = app_cfg("\tone\n\t\ttwo\nx\ty\tz\n", cfg);
    shot("tabs_width_4", &mut app);
}

#[test]
fn modeless_with_unsaved_marker() {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    let (mut app, _) = app_cfg("plain text\n", cfg);
    typed(&mut app, "x");
    shot("modeless_modified", &mut app);
}

#[test]
fn quit_confirmation_prompt() {
    let (mut app, _) = app_with("a");
    press(&mut app, "A");
    typed(&mut app, "b");
    press(&mut app, "esc ctrl+q");
    shot("quit_prompt", &mut app);
}

#[test]
fn tiny_terminal() {
    let (mut app, _) = app_with("tiny\n");
    let buf = render_size(&mut app, 20, 6);
    assert_snapshot("tiny_20x6", &buf);
}

/// A document taller and wider than the window, so both scrollbars show; scrolled into the middle.
fn long_doc() -> String {
    let mut t = String::new();
    for i in 0..120 {
        if i == 7 {
            t.push_str(&format!("// {}\n", "a very long comment line that goes on and on ".repeat(5)));
        } else {
            t.push_str(&format!("func f{i}(x int) int {{ return x + {i} }}\n"));
        }
    }
    t
}

#[test]
fn scrollbars_at_80x24() {
    let (mut app, _) = app_with(&long_doc());
    app.set_lang(Some(&syntax::GO));
    render_size(&mut app, 80, 24);
    app.top_line = 40;
    app.left_col = 30;
    app.follow = false;
    assert_snapshot("scrollbars_80x24", &render_size(&mut app, 80, 24));
}

#[test]
fn scrollbars_at_120x40() {
    let (mut app, _) = app_with(&long_doc());
    app.set_lang(Some(&syntax::GO));
    render_size(&mut app, 120, 40);
    app.top_line = 70;
    app.left_col = 0;
    app.follow = false;
    assert_snapshot("scrollbars_120x40", &render_size(&mut app, 120, 40));
}

#[test]
fn scrollbars_hidden_when_the_text_fits_at_80x24() {
    let (mut app, _) = app_with("short\nfile\n");
    assert_snapshot("scrollbars_hidden_80x24", &render_size(&mut app, 80, 24));
}

// ---------------------------------------------------------------- one snapshot per language mode (task Features 2 and 4)

fn lang_shot(name: &str, mode: &str, src: &str) {
    let (mut app, _) = app_with(src);
    app.set_lang(Some(syntax::by_name(mode).unwrap()));
    shot(name, &mut app);
}

macro_rules! lang_snapshot {
    ($test:ident, $file:literal, $mode:literal, $src:expr) => {
        #[test]
        fn $test() {
            lang_shot($file, $mode, $src);
        }
    };
}

lang_snapshot!(
    lang_lua,
    "lang_lua",
    "lua",
    "-- count\nlocal function f(t)\n  for i, v in ipairs(t) do\n    if v ~= nil and i >= 0x10 then print(\"v\" .. v, #t) end\n  end\nend\n--[[ block\ncomment ]]\nreturn { 1, 2.5e3 }\n"
);
lang_snapshot!(
    lang_rust,
    "lang_rust",
    "rust",
    "// parse\nfn first<'a>(xs: &'a [u8]) -> Option<&'a u8> {\n    let c = 'x';\n    match xs.get(0) { Some(b) if *b >= 0x20 => Some(b), _ => None }\n}\nstruct P { n: u32, s: String } /* data */\nlet r = 1..=10; let t = \"a\\\"b\";\n"
);
lang_snapshot!(
    lang_c,
    "lang_c",
    "c",
    "#include <stdio.h>\n#define MAX 10\n/* main */\nint main(int argc, char **argv) {\n  for (int i = 0; i < MAX; i++) { // loop\n    printf(\"%d\\n\", i * 2);\n  }\n  return 0x0;\n}\n"
);
lang_snapshot!(
    lang_cpp,
    "lang_cpp",
    "c++",
    "#include <vector>\nnamespace app {\ntemplate <typename T> class Box : public Base {\npublic:\n  virtual void put(const T& v) override; // store\n  std::vector<T> items = {};\n};\n}\nauto p = nullptr; int n = 42;\n"
);
lang_snapshot!(
    lang_java,
    "lang_java",
    "java",
    "package app;\n\n@Entity\npublic class User {\n  private final String name = \"kuro\";\n  /* id */ private long id = 42L;\n  public static void main(String[] args) { // go\n    if (args.length >= 1 && args[0] != null) { System.out.println(args[0]); }\n  }\n}\n"
);
lang_snapshot!(
    lang_javascript,
    "lang_javascript",
    "javascript",
    "// utils\nconst re = /^[a-z]+\\d*$/gi;\nasync function load(url, n = 0b101) {\n  const t = `url: ${url}`;\n  return fetch(url).then((r) => r.json()) ?? null; /* done */\n}\nlet half = total / 2 / count;\n"
);
lang_snapshot!(
    lang_typescript,
    "lang_typescript",
    "typescript",
    "interface User { id: number; name?: string }\nexport type Id = keyof User;\nconst u: User = { id: 1, name: 'kuro' }; // ok\nfunction f<T extends object>(x: T): readonly T[] { return [x]; }\n"
);
lang_snapshot!(
    lang_html,
    "lang_html",
    "html",
    "<!DOCTYPE html>\n<!-- page -->\n<html lang=\"en\">\n<style>\n  .box:hover { color: #c00; margin: 0 1.5em; }\n</style>\n<body class=main>\n  <a href='x' disabled>save &amp; go</a>\n  <script>\n    const n = /ab+/.test(s); // js\n  </script>\n</body>\n</html>\n"
);
lang_snapshot!(
    lang_css,
    "lang_css",
    "css",
    "@media (max-width: 600px) {\n  .nav > li:first-child, #id[data-x=\"y\"] {\n    background: url(\"a.png\") no-repeat;\n    margin: 0 auto !important; /* reset */\n    --main: #FF00aa; width: calc(100% - 2rem);\n  }\n}\nbody { font-family: \"Helvetica Neue\", sans-serif }\n"
);
lang_snapshot!(
    lang_php,
    "lang_php",
    "php",
    "<html>\n<?php\n// greet\n$name = \"kuro\"; echo $name . 42;\nif ($a >= 1) { return null; } # note\n?>\n<p><?= $x ?></p>\n<?php foreach ($xs as $x): ?>\n<b>x</b>\n<?php endforeach; ?>\n"
);
lang_snapshot!(
    lang_ruby,
    "lang_ruby",
    "ruby",
    "# model\nclass User < Base\n  attr_accessor :name\n  def initialize(name = nil)\n    @name = name || 'anon'; @n = 3.14\n  end\n  def to_s = puts \"hi #{@name}\"\nend\n"
);
lang_snapshot!(
    lang_yaml,
    "lang_yaml",
    "yaml",
    "# config\nname: kuro\nitems:\n  - key: 'v'  # note\n  - 42\nflag: true\nurl: http://x:80/y\nlist: [1, 2, three]\n\"quoted\": null\n"
);
lang_snapshot!(
    lang_toml,
    "lang_toml",
    "toml",
    "# manifest\n[package]\nname = \"kuro\"  # trailing\nversion = '0.1'\nn = 1_000\nok = true\n\n[dependencies.ropey]\nlist = [1, 2.5, \"x\"]\ndoc = \"\"\"multi\nline\"\"\"\n"
);
lang_snapshot!(
    lang_xml,
    "lang_xml",
    "xml",
    "<?xml version=\"1.0\"?>\n<!-- c -->\n<root xmlns:a=\"u\">\n  <a:item id='1'/>\n  <![CDATA[ <raw> ]]>\n  text &lt; more\n</root>\n"
);
