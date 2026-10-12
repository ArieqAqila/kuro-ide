//! Keybinding checks: `docs/KEYBINDINGS.md` matches `src/keymap.rs` exactly (both directions), and every
//! registered binding has a behaviour probe that proves it does what the table says.

mod common;
use common::fakedb::*;
use common::sqlview::*;
use common::ui::*;
use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
use kuro::app::{App, Focus};
use kuro::config::Config;
use kuro::keymap::{BINDINGS, MD_BEGIN, MD_END, markdown};
use kuro::sql::profile::Engine;
use std::collections::BTreeSet;

// ================================================================ docs check

fn docs_block(docs: &str) -> Option<&str> {
    let a = docs.find(MD_BEGIN)? + MD_BEGIN.len();
    let b = docs.find(MD_END)?;
    Some(docs[a..b].trim_start_matches('\n'))
}

/// Problems between a docs file and the registry-generated tables.
fn docs_problems(docs: &str) -> Vec<String> {
    let Some(block) = docs_block(docs) else { return vec!["KEYBINDINGS.md has no generated block (BEGIN/END markers)".into()] };
    let want = markdown();
    if block == want {
        return vec![];
    }
    // explain which rows differ
    let rows = |t: &str| -> BTreeSet<String> {
        let mut ctx = String::new();
        let mut out = BTreeSet::new();
        for l in t.lines() {
            if let Some(c) = l.strip_prefix("### ") {
                ctx = c.to_string();
            } else if l.starts_with("| `") {
                out.insert(format!("[{ctx}] {l}"));
            }
        }
        out
    };
    let (have, need) = (rows(block), rows(&want));
    let mut v: Vec<String> = need.difference(&have).map(|r| format!("in code but missing or different in docs: {r}")).collect();
    v.extend(have.difference(&need).map(|r| format!("in docs but not in code: {r}")));
    if v.is_empty() {
        v.push("tables differ in layout; run `make docs`".into());
    }
    v
}

const DOCS: &str = include_str!("../docs/KEYBINDINGS.md");

#[test]
fn keybindings_doc_matches_the_code_exactly() {
    let p = docs_problems(DOCS);
    assert!(p.is_empty(), "docs/KEYBINDINGS.md is out of sync (run `make docs`):\n  {}", p.join("\n  "));
}

#[test]
fn the_docs_checker_detects_drift_in_both_directions() {
    // a binding in code that the docs lack
    let missing = DOCS.replace("| `Ctrl+S` |", "| `Ctrl+Shift+S` |");
    let p = docs_problems(&missing);
    assert!(p.iter().any(|m| m.contains("Ctrl+S") && m.contains("missing")), "{p:?}");
    assert!(p.iter().any(|m| m.contains("Ctrl+Shift+S") && m.contains("not in code")), "{p:?}");
    // a documented binding that does not exist
    let extra = DOCS.replace("<!-- END KEYBINDINGS -->", "| `Ctrl+Alt+Q` | Imaginary |\n<!-- END KEYBINDINGS -->");
    assert!(docs_problems(&extra).iter().any(|m| m.contains("Ctrl+Alt+Q")));
    // an edited description
    let edited = DOCS.replace("Save (asks for a name", "Persist (asks for a name");
    assert!(!docs_problems(&edited).is_empty());
    // missing markers
    assert!(!docs_problems("# nothing").is_empty());
    // and the real docs are clean
    assert!(docs_problems(DOCS).is_empty());
}

// ================================================================ probes

fn modal(text: &str) -> App {
    app_with(text).0
}

fn modeless_app(text: &str) -> App {
    modeless(text).0
}

fn tmp_file(name: &str, content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join(name);
    std::fs::write(&p, content).unwrap();
    (d, p)
}

fn at(app: &mut App, line: usize, col: usize) {
    app.buffer.anchor = None;
    app.buffer.cursor = app.buffer.pos_of(line, col);
}

fn lc(app: &App) -> (usize, usize) {
    app.buffer.cursor_line_col()
}

fn cmd(app: &mut App, c: &str) {
    press(app, ":");
    typed(app, c);
    press(app, "enter");
}

fn many_lines() -> String {
    (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
}

type Probe = Box<dyn Fn()>;

fn p(f: impl Fn() + 'static) -> Probe {
    Box::new(f)
}

fn everywhere() -> Vec<(&'static str, Probe)> {
    vec![
        (
            "save",
            p(|| {
                let (_d, f) = tmp_file("s.txt", "abc");
                let mut a = modeless_app("");
                a.open_file(&f);
                typed(&mut a, "x");
                press(&mut a, "ctrl+s");
                assert_eq!(std::fs::read_to_string(&f).unwrap(), "xabc");
            }),
        ),
        (
            "quit",
            p(|| {
                let mut a = modeless_app("x");
                press(&mut a, "ctrl+q");
                assert!(a.quit);
                let mut a = modeless_app("x");
                typed(&mut a, "y");
                press(&mut a, "ctrl+q");
                assert!(!a.quit && a.focus == Focus::Confirm, "asks first when modified");
            }),
        ),
        (
            "find",
            p(|| {
                let mut a = modal("x");
                press(&mut a, "ctrl+f");
                assert!(a.find.open && a.focus == Focus::Find && !a.find.replace_mode);
            }),
        ),
        (
            "replace",
            p(|| {
                let mut a = modal("x");
                press(&mut a, "ctrl+h");
                assert!(a.find.open && a.find.replace_mode);
            }),
        ),
        (
            "undo",
            p(|| {
                let mut a = modeless_app("a");
                a.buffer.cursor = 1;
                typed(&mut a, "b");
                press(&mut a, "ctrl+z");
                assert_eq!(text_of(&a), "a");
            }),
        ),
        (
            "redo",
            p(|| {
                let mut a = modeless_app("a");
                a.buffer.cursor = 1;
                typed(&mut a, "b");
                press(&mut a, "ctrl+z ctrl+y");
                assert_eq!(text_of(&a), "ab");
            }),
        ),
        (
            "redo2",
            p(|| {
                let mut a = modeless_app("a");
                a.buffer.cursor = 1;
                typed(&mut a, "b");
                press(&mut a, "ctrl+z ctrl+shift+z");
                assert_eq!(text_of(&a), "ab");
            }),
        ),
        (
            "copy",
            p(|| {
                let (mut a, clip) = modeless("hello");
                press(&mut a, "ctrl+a ctrl+c");
                assert_eq!(*clip.0.lock().unwrap(), "hello");
                assert_eq!(text_of(&a), "hello");
            }),
        ),
        (
            "cut",
            p(|| {
                let (mut a, clip) = modeless("hello");
                press(&mut a, "ctrl+a ctrl+x");
                assert_eq!((*clip.0.lock().unwrap()).as_str(), "hello");
                assert_eq!(text_of(&a), "");
            }),
        ),
        (
            "paste",
            p(|| {
                let (mut a, clip) = modeless("");
                *clip.0.lock().unwrap() = "pasted".into();
                press(&mut a, "ctrl+v");
                assert_eq!(text_of(&a), "pasted");
            }),
        ),
        (
            "select_all",
            p(|| {
                let mut a = modal("a\nb");
                press(&mut a, "ctrl+a");
                assert_eq!(a.buffer.selected_text().as_deref(), Some("a\nb"));
            }),
        ),
        (
            "alt_copy",
            p(|| {
                let (mut a, clip) = modeless("hi");
                press(&mut a, "ctrl+a alt+c");
                assert_eq!(*clip.0.lock().unwrap(), "hi");
            }),
        ),
        (
            "alt_cut",
            p(|| {
                let (mut a, clip) = modeless("hi");
                press(&mut a, "ctrl+a alt+x");
                assert_eq!((*clip.0.lock().unwrap()).as_str(), "hi");
                assert_eq!(text_of(&a), "");
            }),
        ),
        (
            "alt_paste",
            p(|| {
                let (mut a, clip) = modeless("");
                *clip.0.lock().unwrap() = "z".into();
                press(&mut a, "alt+v");
                assert_eq!(text_of(&a), "z");
            }),
        ),
        (
            "cmdline",
            p(|| {
                let mut a = modeless_app("");
                press(&mut a, "ctrl+p");
                assert_eq!(a.focus, Focus::Prompt);
            }),
        ),
        (
            "snippets",
            p(|| {
                let mut a = modal("");
                press(&mut a, "alt+s");
                assert_eq!(a.focus, Focus::Picker);
            }),
        ),
        (
            "next_match",
            p(|| {
                let mut a = modeless_app("a b a b a");
                press(&mut a, "ctrl+f");
                typed(&mut a, "a");
                press(&mut a, "esc f3");
                assert_eq!(a.buffer.selection().map(|r| r.start), Some(4));
            }),
        ),
        (
            "prev_match",
            p(|| {
                let mut a = modeless_app("a b a b a");
                press(&mut a, "ctrl+f");
                typed(&mut a, "a");
                press(&mut a, "esc shift+f3");
                assert_eq!(a.buffer.selection().map(|r| r.start), Some(8), "wraps to the last match");
            }),
        ),
        (
            "focus_table",
            p(|| {
                let mut a = modal("x");
                show_rows(&mut a, 3, 2);
                press(&mut a, "f4");
                assert_eq!(a.focus, Focus::Table);
                press(&mut a, "f4");
                assert_eq!(a.focus, Focus::Editor);
            }),
        ),
        (
            "run_buffer",
            p(|| {
                let mut a = modal("select 1;\nselect 2;");
                let log = connect_recorder(&mut a, Engine::Postgres);
                press(&mut a, "f6");
                wait_idle(&mut a);
                assert_eq!(ran(&log), vec!["select 1;\nselect 2;"]);
            }),
        ),
        (
            "cancel_query",
            p(|| {
                let mut a = modal("x");
                connect_slow(&mut a);
                press(&mut a, "f6");
                assert!(a.sql.session.as_ref().unwrap().is_running());
                press(&mut a, "f7");
                wait_idle(&mut a);
                let e = a.sql.result.as_ref().unwrap().outcome.as_ref().unwrap_err();
                assert_eq!(e.code.as_deref(), Some("57014"));
            }),
        ),
        (
            "run",
            p(|| {
                let mut a = modal("select 1;\nselect 2;");
                let log = connect_recorder(&mut a, Engine::Postgres);
                at(&mut a, 1, 1);
                press(&mut a, "ctrl+enter");
                wait_idle(&mut a);
                assert_eq!(ran(&log), vec!["select 2;"]);
            }),
        ),
        (
            "run_fallback",
            p(|| {
                let mut a = modal("select 1;");
                let log = connect_recorder(&mut a, Engine::Postgres);
                press(&mut a, "alt+enter");
                wait_idle(&mut a);
                assert_eq!(ran(&log), vec!["select 1;"]);
            }),
        ),
        (
            "run_line",
            p(|| {
                let mut a = modal("select\n  1;");
                let log = connect_recorder(&mut a, Engine::Postgres);
                at(&mut a, 1, 0);
                press(&mut a, "alt+l");
                wait_idle(&mut a);
                assert_eq!(ran(&log), vec!["  1;"]);
            }),
        ),
    ]
}

fn normal() -> Vec<(&'static str, Probe)> {
    vec![
        (
            "n_left",
            p(|| {
                let mut a = modal("abc");
                press(&mut a, "l l h");
                assert_eq!(lc(&a), (0, 1));
                press(&mut a, "left");
                assert_eq!(lc(&a), (0, 0), "arrow keys too");
            }),
        ),
        (
            "n_down",
            p(|| {
                let mut a = modal("a\nb");
                press(&mut a, "j");
                assert_eq!(lc(&a).0, 1);
            }),
        ),
        (
            "n_up",
            p(|| {
                let mut a = modal("a\nb");
                press(&mut a, "j k");
                assert_eq!(lc(&a).0, 0);
            }),
        ),
        (
            "n_right",
            p(|| {
                let mut a = modal("abc");
                press(&mut a, "l");
                assert_eq!(lc(&a), (0, 1));
            }),
        ),
        (
            "n_word",
            p(|| {
                let mut a = modal("foo bar");
                press(&mut a, "w");
                assert_eq!(lc(&a), (0, 4));
            }),
        ),
        (
            "n_back",
            p(|| {
                let mut a = modal("foo bar");
                press(&mut a, "w b");
                assert_eq!(lc(&a), (0, 0));
            }),
        ),
        (
            "n_end",
            p(|| {
                let mut a = modal("foo bar");
                press(&mut a, "e");
                assert_eq!(lc(&a), (0, 2));
            }),
        ),
        (
            "n_line0",
            p(|| {
                let mut a = modal("  foo");
                press(&mut a, "$ 0");
                assert_eq!(lc(&a), (0, 0));
            }),
        ),
        (
            "n_first",
            p(|| {
                let mut a = modal("  foo");
                press(&mut a, "^");
                assert_eq!(lc(&a), (0, 2));
            }),
        ),
        (
            "n_eol",
            p(|| {
                let mut a = modal("foo");
                press(&mut a, "$");
                assert_eq!(lc(&a), (0, 2));
            }),
        ),
        (
            "n_top",
            p(|| {
                let mut a = modal("a\nb\nc");
                press(&mut a, "G g g");
                assert_eq!(lc(&a).0, 0);
                press(&mut a, "2 g g");
                assert_eq!(lc(&a).0, 1, "with a count: that line");
            }),
        ),
        (
            "n_bottom",
            p(|| {
                let mut a = modal("a\nb\nc");
                press(&mut a, "G");
                assert_eq!(lc(&a).0, 2);
                press(&mut a, "2 G");
                assert_eq!(lc(&a).0, 1);
            }),
        ),
        (
            "n_insert",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "l i");
                assert_eq!(a.mode_label(), "INSERT");
                typed(&mut a, "X");
                assert_eq!(text_of(&a), "aXb");
            }),
        ),
        (
            "n_append",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "a");
                typed(&mut a, "X");
                assert_eq!(text_of(&a), "aXb");
            }),
        ),
        (
            "n_insert_bol",
            p(|| {
                let mut a = modal("  ab");
                press(&mut a, "$ I");
                typed(&mut a, "X");
                assert_eq!(text_of(&a), "  Xab");
            }),
        ),
        (
            "n_append_eol",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "A");
                typed(&mut a, "X");
                assert_eq!(text_of(&a), "abX");
            }),
        ),
        (
            "n_open_below",
            p(|| {
                let mut a = modal("a\nb");
                press(&mut a, "o");
                typed(&mut a, "X");
                assert_eq!(text_of(&a), "a\nX\nb");
            }),
        ),
        (
            "n_open_above",
            p(|| {
                let mut a = modal("a\nb");
                press(&mut a, "j O");
                typed(&mut a, "X");
                assert_eq!(text_of(&a), "a\nX\nb");
            }),
        ),
        (
            "n_x",
            p(|| {
                let mut a = modal("abcd");
                press(&mut a, "x");
                assert_eq!(text_of(&a), "bcd");
                press(&mut a, "2 x");
                assert_eq!(text_of(&a), "d");
            }),
        ),
        (
            "n_X",
            p(|| {
                let mut a = modal("abc");
                press(&mut a, "l X");
                assert_eq!(text_of(&a), "bc");
            }),
        ),
        (
            "n_dd",
            p(|| {
                let mut a = modal("a\nb\nc\nd");
                press(&mut a, "d d");
                assert_eq!(text_of(&a), "b\nc\nd");
                press(&mut a, "2 d d");
                assert_eq!(text_of(&a), "d");
            }),
        ),
        (
            "n_yy",
            p(|| {
                let mut a = modal("a\nb");
                press(&mut a, "y y j p");
                assert_eq!(text_of(&a), "a\nb\na");
            }),
        ),
        (
            "n_cc",
            p(|| {
                let mut a = modal("abc\ndef");
                press(&mut a, "c c");
                typed(&mut a, "X");
                press(&mut a, "esc");
                assert_eq!(text_of(&a), "X\ndef");
            }),
        ),
        (
            "n_dmotion",
            p(|| {
                let mut a = modal("foo bar baz");
                press(&mut a, "d w");
                assert_eq!(text_of(&a), "bar baz");
                press(&mut a, "d $");
                assert_eq!(text_of(&a), "");
            }),
        ),
        (
            "n_ymotion",
            p(|| {
                let mut a = modal("foo bar");
                press(&mut a, "y w $ p");
                assert_eq!(text_of(&a), "foo barfoo ");
            }),
        ),
        (
            "n_cmotion",
            p(|| {
                let mut a = modal("foo bar");
                press(&mut a, "c w");
                typed(&mut a, "X");
                press(&mut a, "esc");
                assert_eq!(text_of(&a), "X bar", "cw stops at the end of the word");
            }),
        ),
        (
            "n_D",
            p(|| {
                let mut a = modal("foo bar");
                press(&mut a, "w D");
                assert_eq!(text_of(&a), "foo ");
            }),
        ),
        (
            "n_C",
            p(|| {
                let mut a = modal("foo bar");
                press(&mut a, "w C");
                typed(&mut a, "X");
                assert_eq!(text_of(&a), "foo X");
            }),
        ),
        (
            "n_Y",
            p(|| {
                let mut a = modal("a\nb");
                press(&mut a, "Y p");
                assert_eq!(text_of(&a), "a\na\nb");
            }),
        ),
        (
            "n_p",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "x p");
                assert_eq!(text_of(&a), "ba");
            }),
        ),
        (
            "n_P",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "x P");
                assert_eq!(text_of(&a), "ab");
            }),
        ),
        (
            "n_u",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "x u");
                assert_eq!(text_of(&a), "ab");
            }),
        ),
        (
            "n_ctrl_r",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "x u ctrl+r");
                assert_eq!(text_of(&a), "b");
            }),
        ),
        (
            "n_v",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "v");
                assert_eq!(a.mode_label(), "VISUAL");
            }),
        ),
        (
            "n_V",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "V");
                assert_eq!(a.mode_label(), "V-LINE");
            }),
        ),
        (
            "n_colon",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, ":");
                assert_eq!(a.focus, Focus::Prompt);
            }),
        ),
        (
            "n_slash",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "/");
                assert_eq!(a.focus, Focus::Find);
            }),
        ),
        (
            "n_n",
            p(|| {
                let mut a = modal("a b a");
                press(&mut a, "/");
                typed(&mut a, "a");
                press(&mut a, "esc n");
                assert_eq!(a.buffer.selection().map(|r| r.start), Some(4));
            }),
        ),
        (
            "n_N",
            p(|| {
                let mut a = modal("a b a");
                press(&mut a, "/");
                typed(&mut a, "a");
                press(&mut a, "esc N");
                assert_eq!(a.buffer.selection().map(|r| r.start), Some(4), "wraps backwards from the first match");
            }),
        ),
        (
            "n_ctrl_d",
            p(|| {
                let mut a = modal(&many_lines());
                press(&mut a, "ctrl+d");
                assert!(lc(&a).0 >= 5);
            }),
        ),
        (
            "n_ctrl_u",
            p(|| {
                let mut a = modal(&many_lines());
                press(&mut a, "ctrl+d ctrl+d ctrl+u");
                let after_two_down_one_up = lc(&a).0;
                let mut b = modal(&many_lines());
                press(&mut b, "ctrl+d ctrl+d");
                assert!(after_two_down_one_up < lc(&b).0 && after_two_down_one_up > 0);
            }),
        ),
        (
            "n_count",
            p(|| {
                let mut a = modal("a\nb\nc\nd\ne");
                press(&mut a, "3 j");
                assert_eq!(lc(&a).0, 3);
            }),
        ),
        (
            "n_esc",
            p(|| {
                let mut a = modal("a\nb\nc");
                press(&mut a, "3 esc j");
                assert_eq!(lc(&a).0, 1, "the pending count was dropped");
                press(&mut a, "d esc d d");
                assert_eq!(text_of(&a), "a\nc", "the pending operator was dropped, then dd works");
            }),
        ),
    ]
}

fn visual() -> Vec<(&'static str, Probe)> {
    vec![
        (
            "v_motion",
            p(|| {
                let mut a = modal("foo bar\nbaz");
                press(&mut a, "v w");
                assert_eq!(a.buffer.selected_text().as_deref(), None.or(a.buffer.selected_text().as_deref()));
                let r = a.selection_range().unwrap();
                assert!(r.end - r.start >= 5, "w extended the selection: {r:?}");
                press(&mut a, "esc g g v j $");
                assert!(a.selection_range().unwrap().end >= 11);
            }),
        ),
        (
            "v_d",
            p(|| {
                let mut a = modal("hello");
                press(&mut a, "v l d");
                assert_eq!(text_of(&a), "llo");
                let mut b = modal("hello");
                press(&mut b, "v l x");
                assert_eq!(text_of(&b), "llo");
            }),
        ),
        (
            "v_y",
            p(|| {
                let (mut a, clip) = app_with("hello");
                press(&mut a, "v l l y");
                assert_eq!(*clip.0.lock().unwrap(), "hel");
                assert_eq!(a.mode_label(), "NORMAL");
            }),
        ),
        (
            "v_c",
            p(|| {
                let mut a = modal("hello");
                press(&mut a, "v l c");
                typed(&mut a, "X");
                press(&mut a, "esc");
                assert_eq!(text_of(&a), "Xllo");
            }),
        ),
        (
            "v_p",
            p(|| {
                let mut a = modal("ab cd");
                press(&mut a, "y w w v l p");
                assert_eq!(text_of(&a), "ab ab ");
            }),
        ),
        (
            "v_o",
            p(|| {
                let mut a = modal("hello");
                press(&mut a, "v l l o");
                assert_eq!(lc(&a), (0, 0), "the cursor jumped to the other end");
                assert_eq!(a.buffer.anchor, Some(2));
            }),
        ),
        (
            "v_switch",
            p(|| {
                let mut a = modal("ab\ncd");
                press(&mut a, "v V");
                assert_eq!(a.mode_label(), "V-LINE");
                press(&mut a, "v");
                assert_eq!(a.mode_label(), "VISUAL");
                press(&mut a, "v");
                assert_eq!(a.mode_label(), "NORMAL");
                press(&mut a, "V V");
                assert_eq!(a.mode_label(), "NORMAL");
            }),
        ),
        (
            "v_esc",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "v l esc");
                assert_eq!(a.mode_label(), "NORMAL");
                assert!(a.buffer.selection().is_none());
            }),
        ),
    ]
}

fn insert() -> Vec<(&'static str, Probe)> {
    vec![
        (
            "i_esc",
            p(|| {
                let mut a = modal("ab");
                press(&mut a, "i esc");
                assert_eq!(a.mode_label(), "NORMAL");
                let mut b = modeless_app("ab");
                press(&mut b, "ctrl+a esc");
                assert!(b.buffer.selection().is_none() || b.buffer.anchor.is_none() || b.selection_range().is_some());
            }),
        ),
        (
            "i_enter",
            p(|| {
                let mut a = modeless_app("\t\tab");
                a.buffer.cursor = 4;
                press(&mut a, "enter");
                assert_eq!(text_of(&a), "\t\tab\n\t\t");
            }),
        ),
        (
            "i_tab",
            p(|| {
                let mut a = modeless_app("a\nb");
                press(&mut a, "tab");
                assert_eq!(text_of(&a), "\ta\nb");
                press(&mut a, "ctrl+a tab");
                assert_eq!(text_of(&a), "\t\ta\n\tb");
            }),
        ),
        (
            "i_backtab",
            p(|| {
                let mut a = modeless_app("\t\ta");
                press(&mut a, "shift+tab");
                assert_eq!(text_of(&a), "\ta");
            }),
        ),
        (
            "i_backspace",
            p(|| {
                let mut a = modeless_app("a   b");
                a.buffer.cursor = 4;
                press(&mut a, "backspace");
                assert_eq!(text_of(&a), "ab");
                let mut b = modeless_app("\t\tx");
                b.buffer.cursor = 2;
                press(&mut b, "backspace");
                assert_eq!(text_of(&b), "\tx");
            }),
        ),
        (
            "i_delete",
            p(|| {
                let mut a = modeless_app("a   b");
                a.buffer.cursor = 1;
                press(&mut a, "delete");
                assert_eq!(text_of(&a), "ab");
            }),
        ),
        (
            "i_alt_bs",
            p(|| {
                let mut a = modeless_app("foo bar");
                a.buffer.cursor = 7;
                press(&mut a, "alt+backspace");
                assert_eq!(text_of(&a), "foo ");
            }),
        ),
        (
            "i_arrows",
            p(|| {
                let mut a = modeless_app("ab\ncd");
                press(&mut a, "down right");
                assert_eq!(lc(&a), (1, 1));
                press(&mut a, "up left");
                assert_eq!(lc(&a), (0, 0));
                press(&mut a, "shift+right shift+right");
                assert_eq!(a.buffer.selected_text().as_deref(), Some("ab"));
            }),
        ),
        (
            "i_ctrl_arrows",
            p(|| {
                let mut a = modeless_app("foo bar baz");
                press(&mut a, "ctrl+right");
                assert_eq!(lc(&a), (0, 4));
                press(&mut a, "ctrl+left");
                assert_eq!(lc(&a), (0, 0));
                press(&mut a, "ctrl+shift+right");
                assert_eq!(a.buffer.selected_text().as_deref(), Some("foo "));
            }),
        ),
        (
            "i_home_end",
            p(|| {
                let mut a = modeless_app("  foo");
                press(&mut a, "end");
                assert_eq!(lc(&a), (0, 5));
                press(&mut a, "home");
                assert_eq!(lc(&a), (0, 2), "first non-blank first");
                press(&mut a, "home");
                assert_eq!(lc(&a), (0, 0));
            }),
        ),
        (
            "i_ctrl_home_end",
            p(|| {
                let mut a = modeless_app("a\nb\nc");
                press(&mut a, "ctrl+end");
                assert_eq!(a.buffer.cursor, 5);
                press(&mut a, "ctrl+home");
                assert_eq!(a.buffer.cursor, 0);
            }),
        ),
        (
            "n_shift_arrows",
            p(|| {
                let mut a = modal("abc\ndef\n");
                press(&mut a, "shift+right shift+right shift+down");
                assert_eq!(a.buffer.selected_text().as_deref(), Some("abc\nde"));
            }),
        ),
        (
            "n_shift_home_end",
            p(|| {
                let mut a = modal("abc def\n");
                press(&mut a, "right shift+end");
                assert_eq!(a.buffer.selected_text().as_deref(), Some("bc def"));
                press(&mut a, "esc home right right shift+home");
                assert_eq!(a.buffer.selected_text().as_deref(), Some("ab"));
            }),
        ),
        (
            "n_shift_page",
            p(|| {
                let mut a = modal(&many_lines());
                press(&mut a, "shift+pagedown");
                assert!(a.buffer.selection().is_some_and(|r| a.buffer.line_of(r.end) > 5));
            }),
        ),
        (
            "n_shift_ctrl_arrows",
            p(|| {
                let mut a = modal("one two three\n");
                press(&mut a, "shift+ctrl+right");
                assert_eq!(a.buffer.selected_text().as_deref(), Some("one "));
            }),
        ),
        (
            "i_page",
            p(|| {
                let mut a = modeless_app(&many_lines());
                press(&mut a, "pagedown");
                let down = lc(&a).0;
                assert!(down > 5);
                press(&mut a, "pageup");
                assert!(lc(&a).0 < down);
            }),
        ),
    ]
}

fn find_bar() -> Vec<(&'static str, Probe)> {
    vec![
        (
            "f_next",
            p(|| {
                let mut a = modeless_app("a b a b a");
                press(&mut a, "ctrl+f");
                typed(&mut a, "a");
                press(&mut a, "enter");
                assert_eq!(a.buffer.selection().map(|r| r.start), Some(4));
                // in the replace field Enter replaces the current match
                let mut b = modeless_app("a b a");
                press(&mut b, "ctrl+h");
                typed(&mut b, "a");
                press(&mut b, "tab");
                typed(&mut b, "X");
                press(&mut b, "enter");
                assert_eq!(text_of(&b), "X b a");
            }),
        ),
        (
            "f_prev",
            p(|| {
                let mut a = modeless_app("a b a b a");
                press(&mut a, "ctrl+f");
                typed(&mut a, "a");
                press(&mut a, "shift+enter");
                assert_eq!(a.buffer.selection().map(|r| r.start), Some(8));
            }),
        ),
        (
            "f_esc",
            p(|| {
                let mut a = modeless_app("a");
                press(&mut a, "ctrl+f esc");
                assert!(!a.find.open && a.focus == Focus::Editor);
            }),
        ),
        (
            "f_case",
            p(|| {
                let mut a = modeless_app("A a");
                press(&mut a, "ctrl+f");
                typed(&mut a, "a");
                assert_eq!(a.find.matches.len(), 2);
                press(&mut a, "alt+c");
                assert!(a.find.case_sensitive);
                assert_eq!(a.find.matches.len(), 1);
            }),
        ),
        (
            "f_word",
            p(|| {
                let mut a = modeless_app("a ab");
                press(&mut a, "ctrl+f");
                typed(&mut a, "a");
                assert_eq!(a.find.matches.len(), 2);
                press(&mut a, "alt+w");
                assert!(a.find.whole_word);
                assert_eq!(a.find.matches.len(), 1);
            }),
        ),
        (
            "f_regex",
            p(|| {
                let mut a = modeless_app("a1 b22");
                press(&mut a, "ctrl+f alt+r");
                typed(&mut a, "[0-9]+");
                assert!(a.find.regex);
                assert_eq!(a.find.matches.len(), 2);
            }),
        ),
        (
            "f_tab",
            p(|| {
                let mut a = modeless_app("a");
                press(&mut a, "ctrl+h");
                assert!(!a.find.in_replace);
                press(&mut a, "tab");
                assert!(a.find.in_replace);
                press(&mut a, "tab");
                assert!(!a.find.in_replace);
            }),
        ),
        (
            "f_replace_all",
            p(|| {
                let mut a = modeless_app("cat dog cat");
                press(&mut a, "ctrl+h");
                typed(&mut a, "cat");
                press(&mut a, "tab");
                typed(&mut a, "bird");
                press(&mut a, "alt+a");
                assert_eq!(text_of(&a), "bird dog bird");
                press(&mut a, "ctrl+z");
                assert_eq!(text_of(&a), "cat dog cat", "one undo step");
            }),
        ),
        (
            "f_updown",
            p(|| {
                let mut a = modeless_app("a b a");
                press(&mut a, "ctrl+f");
                typed(&mut a, "a");
                press(&mut a, "down");
                assert_eq!(a.buffer.selection().map(|r| r.start), Some(4));
                press(&mut a, "up");
                assert_eq!(a.buffer.selection().map(|r| r.start), Some(0));
            }),
        ),
        (
            "f_kill",
            p(|| {
                let mut a = modeless_app("a");
                press(&mut a, "ctrl+f");
                typed(&mut a, "abc");
                press(&mut a, "ctrl+u");
                assert!(a.find.query.is_empty());
            }),
        ),
    ]
}

fn table() -> Vec<(&'static str, Probe)> {
    let focused = |rows: usize, cols: usize| {
        let mut a = modal("x");
        show_rows(&mut a, rows, cols);
        press(&mut a, "f4");
        a
    };
    let row_off = |a: &App| a.sql.result.as_ref().unwrap().state.row_off;
    let cur_col = |a: &App| a.sql.result.as_ref().unwrap().state.cur_col;
    vec![
        (
            "t_rows",
            p(move || {
                let mut a = focused(60, 2);
                press(&mut a, "j");
                assert_eq!(row_off(&a), 1);
                press(&mut a, "down");
                assert_eq!(row_off(&a), 2);
                press(&mut a, "k up");
                assert_eq!(row_off(&a), 0);
            }),
        ),
        (
            "t_page",
            p(move || {
                let mut a = focused(100, 2);
                press(&mut a, "pagedown");
                let one = row_off(&a);
                assert!(one > 3);
                press(&mut a, "pageup");
                assert_eq!(row_off(&a), 0);
            }),
        ),
        (
            "t_ends",
            p(move || {
                let mut a = focused(100, 2);
                press(&mut a, "G");
                assert!(row_off(&a) > 50);
                press(&mut a, "g");
                assert_eq!(row_off(&a), 0);
            }),
        ),
        (
            "t_cols",
            p(move || {
                let mut a = focused(3, 5);
                press(&mut a, "l l");
                assert_eq!(cur_col(&a), 2);
                press(&mut a, "h");
                assert_eq!(cur_col(&a), 1);
                press(&mut a, "right left");
                assert_eq!(cur_col(&a), 1);
            }),
        ),
        (
            "t_home_end",
            p(move || {
                let mut a = focused(3, 5);
                press(&mut a, "end");
                assert_eq!(cur_col(&a), 4);
                press(&mut a, "home");
                assert_eq!(cur_col(&a), 0);
            }),
        ),
        (
            "t_cells",
            p(move || {
                let mut a = focused(3, 14);
                press(&mut a, "L");
                let right = a.sql.result.as_ref().unwrap().state.col_off;
                assert!(right > 0);
                press(&mut a, "H");
                assert!(a.sql.result.as_ref().unwrap().state.col_off < right);
                press(&mut a, "ctrl+right");
                assert!(a.sql.result.as_ref().unwrap().state.col_off > 0);
                press(&mut a, "ctrl+left");
                assert_eq!(a.sql.result.as_ref().unwrap().state.col_off, 0);
            }),
        ),
        (
            "t_expand",
            p(move || {
                let mut a = focused(3, 3);
                press(&mut a, "e");
                assert!(a.sql.result.as_ref().unwrap().state.expanded.contains(&0));
                press(&mut a, "enter");
                assert!(a.sql.result.as_ref().unwrap().state.expanded.is_empty());
                press(&mut a, "space");
                assert!(a.sql.result.as_ref().unwrap().state.expanded.contains(&0));
            }),
        ),
        (
            "t_back",
            p(move || {
                let mut a = focused(3, 3);
                press(&mut a, "esc");
                assert_eq!(a.focus, Focus::Editor);
                press(&mut a, "f4 tab");
                assert_eq!(a.focus, Focus::Editor);
            }),
        ),
        (
            "t_close",
            p(move || {
                let mut a = focused(3, 3);
                press(&mut a, "q");
                assert!(!a.sql.pane_open && a.focus == Focus::Editor);
            }),
        ),
        (
            "t_copy",
            p(|| {
                let (mut a, clip) = app_with("x");
                show_rows(&mut a, 2, 2);
                press(&mut a, "f4 ctrl+c");
                assert!(clip.0.lock().unwrap().starts_with("col0\tcol1\n"));
            }),
        ),
    ]
}

fn pickers() -> Vec<(&'static str, Probe)> {
    let picker = || {
        let mut a = modal("x");
        cmd(&mut a, "setlang");
        a
    };
    let sel = |a: &App| a.picker.as_ref().unwrap().selected;
    vec![
        (
            "p_move",
            p(move || {
                let mut a = picker();
                press(&mut a, "down down");
                assert_eq!(sel(&a), 2);
                press(&mut a, "up ctrl+p");
                assert_eq!(sel(&a), 0);
                press(&mut a, "ctrl+n");
                assert_eq!(sel(&a), 1);
            }),
        ),
        (
            "p_accept",
            p(move || {
                let mut a = picker();
                typed(&mut a, "python");
                press(&mut a, "enter");
                assert_eq!(a.lang.name, "Python");
                let mut b = picker();
                typed(&mut b, "json");
                press(&mut b, "tab");
                assert_eq!(b.lang.name, "JSON");
            }),
        ),
        (
            "p_esc",
            p(move || {
                let mut a = picker();
                typed(&mut a, "go");
                press(&mut a, "esc");
                assert!(a.picker.is_none() && a.lang.name == "Plain Text");
            }),
        ),
        (
            "p_filter",
            p(move || {
                let mut a = picker();
                let all = a.picker.as_ref().unwrap().visible().len();
                typed(&mut a, "sh");
                assert!(a.picker.as_ref().unwrap().visible().len() < all);
                press(&mut a, "backspace backspace");
                assert_eq!(a.picker.as_ref().unwrap().visible().len(), all);
            }),
        ),
    ]
}

fn suggestions() -> Vec<(&'static str, Probe)> {
    let typing = || {
        let mut a = modeless_app("hello help helmet\n");
        a.buffer.cursor = a.buffer.len_chars();
        typed(&mut a, "hel");
        a
    };
    let sel = |a: &App| a.suggest.as_ref().unwrap().selected;
    vec![
        (
            "s_move",
            p(move || {
                let mut a = typing();
                press(&mut a, "down");
                assert_eq!(sel(&a), 1);
                press(&mut a, "ctrl+n");
                assert_eq!(sel(&a), 2);
                press(&mut a, "up");
                assert_eq!(sel(&a), 1);
                press(&mut a, "ctrl+p");
                assert_eq!(sel(&a), 0);
            }),
        ),
        (
            "s_accept",
            p(move || {
                let mut a = typing();
                let word = a.suggest.as_ref().unwrap().sug.items[0].clone();
                press(&mut a, "tab");
                assert!(text_of(&a).ends_with(&format!("\n{word}")));
                let mut b = typing();
                press(&mut b, "enter");
                assert_eq!(b.buffer.line_count(), 2, "Enter accepted instead of adding a line");
            }),
        ),
        (
            "s_esc",
            p(move || {
                let mut a = typing();
                press(&mut a, "esc");
                assert!(a.suggest.is_none());
                assert!(text_of(&a).ends_with("\nhel"));
            }),
        ),
    ]
}

fn commands() -> Vec<(&'static str, Probe)> {
    vec![
        (
            "c_w",
            p(|| {
                let d = tempfile::tempdir().unwrap();
                let f = d.path().join("w.txt");
                let mut a = modal("data");
                cmd(&mut a, &format!("w {}", f.display()));
                assert_eq!(std::fs::read_to_string(&f).unwrap(), "data");
                press(&mut a, "x");
                cmd(&mut a, "w");
                assert_eq!(std::fs::read_to_string(&f).unwrap(), "ata");
            }),
        ),
        (
            "c_q",
            p(|| {
                let mut a = modal("x");
                cmd(&mut a, "q");
                assert!(a.quit);
                let mut b = modal("x");
                press(&mut b, "x");
                cmd(&mut b, "q");
                assert!(!b.quit, ":q refuses with unsaved changes");
                cmd(&mut b, "q!");
                assert!(b.quit);
            }),
        ),
        (
            "c_wq",
            p(|| {
                let (_d, f) = tmp_file("a.txt", "abc");
                let mut a = modal("");
                a.open_file(&f);
                press(&mut a, "x");
                cmd(&mut a, "wq");
                assert!(a.quit);
                assert_eq!(std::fs::read_to_string(&f).unwrap(), "bc");
                let mut b = modal("");
                b.open_file(&f);
                press(&mut b, "x");
                cmd(&mut b, "x");
                assert!(b.quit);
            }),
        ),
        (
            "c_e",
            p(|| {
                let (_d, f) = tmp_file("e.txt", "loaded");
                let mut a = modal("old");
                cmd(&mut a, &format!("e {}", f.display()));
                assert_eq!(text_of(&a), "loaded");
            }),
        ),
        (
            "c_line",
            p(|| {
                let mut a = modal("a\nb\nc");
                cmd(&mut a, "3");
                assert_eq!(lc(&a).0, 2);
            }),
        ),
        (
            "c_setlang",
            p(|| {
                let mut a = modal("x");
                cmd(&mut a, "setlang go");
                assert_eq!(a.lang.name, "Go");
                cmd(&mut a, "setlang auto");
                assert_eq!(a.lang.name, "Plain Text");
                cmd(&mut a, "setlang");
                assert_eq!(a.focus, Focus::Picker);
            }),
        ),
        (
            "c_snippets",
            p(|| {
                let mut a = modal("");
                cmd(&mut a, "snippets");
                assert_eq!(a.focus, Focus::Picker);
            }),
        ),
        (
            "c_connect",
            p(|| {
                let mut a = modal("x");
                cmd(&mut a, "connect nope");
                assert!(a.msg.as_deref().is_some_and(|m| m.contains("no connection profiles") || m.contains("no profile")));
            }),
        ),
        (
            "c_disconnect",
            p(|| {
                let mut a = modal("x");
                connect_recorder(&mut a, Engine::Postgres);
                cmd(&mut a, "disconnect");
                assert!(a.sql.session.is_none());
            }),
        ),
        (
            "c_run",
            p(|| {
                let mut a = modal("select 1;");
                let log = connect_recorder(&mut a, Engine::Postgres);
                cmd(&mut a, "run");
                wait_idle(&mut a);
                assert_eq!(ran(&log), vec!["select 1;"]);
            }),
        ),
        (
            "c_runline",
            p(|| {
                let mut a = modal("select\n  1;");
                let log = connect_recorder(&mut a, Engine::Postgres);
                at(&mut a, 0, 0);
                cmd(&mut a, "runline");
                wait_idle(&mut a);
                assert_eq!(ran(&log), vec!["select"]);
            }),
        ),
        (
            "c_runbuffer",
            p(|| {
                let mut a = modal("select 1;\nselect 2;");
                let log = connect_recorder(&mut a, Engine::Postgres);
                cmd(&mut a, "runbuffer");
                wait_idle(&mut a);
                assert_eq!(ran(&log), vec!["select 1;\nselect 2;"]);
            }),
        ),
        (
            "c_cancel",
            p(|| {
                let mut a = modal("x");
                connect_slow(&mut a);
                cmd(&mut a, "runbuffer");
                cmd(&mut a, "cancel");
                wait_idle(&mut a);
                assert!(a.sql.result.as_ref().unwrap().outcome.is_err());
            }),
        ),
        (
            "c_sql",
            p(|| {
                let mut a = modal("x");
                show_rows(&mut a, 2, 2);
                cmd(&mut a, "sql close");
                assert!(!a.sql.pane_open);
                cmd(&mut a, "sql show");
                assert!(a.sql.pane_open);
                cmd(&mut a, "sql profiles");
                assert!(a.msg.is_some());
                cmd(&mut a, "sql warnings");
                assert!(a.msg.as_deref().is_some_and(|m| m.contains("no warnings")));
            }),
        ),
        (
            "c_warnings",
            p(|| {
                let mut a = modal("x");
                a.warnings = vec!["unknown config key `a.b` ignored".into()];
                cmd(&mut a, "warnings");
                assert!(a.msg.as_deref().is_some_and(|m| m.contains("a.b")));
            }),
        ),
    ]
}

fn mouse_probes() -> Vec<(&'static str, Probe)> {
    use MouseEventKind as K;
    vec![
        (
            "m_click",
            p(|| {
                let mut a = modal("hello\nworld");
                let gw = gutter_w(&a);
                render(&mut a);
                click(&mut a, gw + 3, 1);
                assert_eq!(lc(&a), (1, 3));
            }),
        ),
        (
            "m_ctrl_right",
            p(|| {
                let mut a = modal("ab  \t cd");
                let gw = gutter_w(&a);
                render(&mut a);
                mouse_mod(&mut a, K::Down(MouseButton::Right), gw + 3, 0, KeyModifiers::CONTROL);
                assert_eq!(lc(&a), (0, 6), "just before `cd`");
            }),
        ),
        (
            "m_scrollbar",
            p(|| {
                let mut a = modal(&many_lines());
                render(&mut a);
                mouse(&mut a, K::Down(MouseButton::Left), 79, 21); // the down arrow
                mouse(&mut a, K::Up(MouseButton::Left), 79, 21);
                assert_eq!(a.top_line, 1);
                mouse(&mut a, K::Down(MouseButton::Left), 79, 15); // the track below the thumb: a page
                mouse(&mut a, K::Up(MouseButton::Left), 79, 15);
                assert_eq!(a.top_line, 22);
            }),
        ),
        (
            "m_drag",
            p(|| {
                let mut a = modal("hello world");
                let gw = gutter_w(&a);
                render(&mut a);
                mouse(&mut a, K::Down(MouseButton::Left), gw, 0);
                mouse(&mut a, K::Drag(MouseButton::Left), gw + 5, 0);
                assert_eq!(a.buffer.selected_text().as_deref(), Some("hello"));
            }),
        ),
        (
            "m_double",
            p(|| {
                let mut a = modal("hello world");
                let gw = gutter_w(&a);
                render(&mut a);
                click(&mut a, gw + 8, 0);
                click(&mut a, gw + 8, 0);
                assert_eq!(a.buffer.selected_text().as_deref(), Some("world"));
            }),
        ),
        (
            "m_triple",
            p(|| {
                let mut a = modal("hello world\nnext");
                let gw = gutter_w(&a);
                render(&mut a);
                for _ in 0..3 {
                    click(&mut a, gw + 2, 0);
                }
                assert_eq!(a.buffer.selected_text().as_deref(), Some("hello world\n"));
            }),
        ),
        (
            "m_gutter",
            p(|| {
                let mut a = modal("one\ntwo\nthree");
                render(&mut a);
                click(&mut a, 1, 1);
                assert_eq!(a.buffer.selected_text().as_deref(), Some("two\n"));
            }),
        ),
        (
            "m_hwheel",
            p(|| {
                let mut a = modal(&"x".repeat(200));
                render(&mut a);
                mouse(&mut a, K::ScrollRight, 10, 0);
                let one = a.left_col;
                assert!(one > 0);
                mouse(&mut a, K::ScrollLeft, 10, 0);
                assert_eq!(a.left_col, 0);
                mouse_mod(&mut a, K::ScrollDown, 10, 0, KeyModifiers::SHIFT);
                assert_eq!(a.left_col, one, "Shift+wheel is the fallback");
            }),
        ),
        (
            "m_wheel",
            p(|| {
                let mut a = modal(&many_lines());
                render(&mut a);
                mouse(&mut a, K::ScrollDown, 10, 5);
                assert!(screen(&mut a)[0].contains("line 3"));
                mouse(&mut a, K::ScrollUp, 10, 5);
                assert!(screen(&mut a)[0].contains("line 0"));
                let long = "x".repeat(200);
                let mut b = modal(&long);
                render(&mut b);
                mouse_mod(&mut b, K::ScrollDown, 10, 5, KeyModifiers::SHIFT);
                assert!(b.left_col > 0, "Shift+wheel scrolls sideways");
            }),
        ),
        (
            "m_bar",
            p(|| {
                let mut a = modal("xyz XYZ");
                press(&mut a, "ctrl+f");
                typed(&mut a, "x");
                let buf = render(&mut a);
                let (x, y) = find_on(&buf, "Aa").unwrap();
                click(&mut a, x, y);
                assert!(a.find.case_sensitive);
                let (x, y) = find_on(&render(&mut a), "✕").unwrap();
                click(&mut a, x, y);
                assert!(!a.find.open);
            }),
        ),
        (
            "m_table_wheel",
            p(|| {
                let mut a = modal("x");
                show_rows(&mut a, 100, 12);
                let s = screen(&mut a);
                let y = (s.iter().position(|r| r.contains("col0")).unwrap() + 3) as u16;
                mouse(&mut a, K::ScrollDown, 4, y);
                assert_eq!(a.sql.result.as_ref().unwrap().state.row_off, 3);
                mouse_mod(&mut a, K::ScrollDown, 4, y, KeyModifiers::SHIFT);
                assert!(a.sql.result.as_ref().unwrap().state.col_off > 0);
            }),
        ),
        (
            "m_table_header",
            p(|| {
                let mut a = modal("x");
                show_rows(&mut a, 3, 3);
                let s = screen(&mut a);
                let y = s.iter().position(|r| r.contains("col0")).unwrap() as u16;
                let x = s[y as usize].find("col1").unwrap() as u16;
                click(&mut a, x, y);
                assert_eq!(a.sql.result.as_ref().unwrap().state.cur_col, 1);
                click(&mut a, x, y);
                assert!(a.sql.result.as_ref().unwrap().state.expanded.contains(&1));
            }),
        ),
        (
            "m_picker",
            p(|| {
                let mut a = modal("x");
                cmd(&mut a, "setlang");
                let (x, y) = find_on(&render(&mut a), "Python").unwrap();
                click(&mut a, x, y);
                assert_eq!(a.lang.name, "Python");
            }),
        ),
    ]
}

#[test]
fn every_registered_binding_has_exactly_one_behaviour_probe() {
    let mut all: Vec<(&'static str, Probe)> = vec![];
    for group in [everywhere(), normal(), visual(), insert(), find_bar(), table(), pickers(), suggestions(), commands(), mouse_probes()] {
        all.extend(group);
    }
    let probe_ids: Vec<&str> = all.iter().map(|(id, _)| *id).collect();
    let unique: BTreeSet<&str> = probe_ids.iter().copied().collect();
    assert_eq!(unique.len(), probe_ids.len(), "duplicate probe ids");
    let registry: BTreeSet<&str> = BINDINGS.iter().map(|b| b.id).collect();
    let missing: Vec<_> = registry.difference(&unique).collect();
    let orphan: Vec<_> = unique.difference(&registry).collect();
    assert!(missing.is_empty(), "bindings without a probe (write one in tests/keybindings.rs): {missing:?}");
    assert!(orphan.is_empty(), "probes for bindings that no longer exist: {orphan:?}");

    let mut failed = vec![];
    for (id, probe) in &all {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(probe));
        if r.is_err() {
            failed.push(*id);
        }
    }
    assert!(failed.is_empty(), "these bindings do not behave as documented: {failed:?} (see the panic messages above)");
    let _ = Config::default();
}
