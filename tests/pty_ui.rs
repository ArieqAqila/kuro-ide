//! End-to-end: the real binary in a pseudo-terminal, screen read through a terminal emulator.
//! Terminal-specific behaviour (a real Wayland clipboard, a particular emulator's mouse quirks)
//! is NOT covered; see README "What is not tested".

mod common;
use common::pty::*;

const CFG: &str = "[clipboard]\nbackend = \"osc52\"\n";

fn sample(dir: &std::path::Path) -> std::path::PathBuf {
    let p = dir.join("sample.txt");
    std::fs::write(&p, "alpha beta\nfoo bar foo\nthird line\nFoo again\n").unwrap();
    p
}

#[test]
fn paints_text_gutter_status_and_never_a_background() {
    let dir = tempfile::tempdir().unwrap();
    let mut pty = Pty::spawn(dir.path(), CFG, Some(&sample(dir.path())), &[]);
    let s = pty.wait_for("first paint", |s| s.iter().any(|r| r.contains("alpha beta")));
    assert!(s[0].contains("alpha beta") && s[0].trim_start().starts_with('1'), "{:?}", s[0]);
    assert!(s[1].trim_start().starts_with('2'), "line numbers are absolute: {:?}", s[1]);
    let status = &s[28];
    assert!(status.contains("NORMAL") && status.contains("sample.txt") && status.contains("Ln 1, Col 1"), "{status:?}");
    pty.assert_default_background();
    // open every overlay and re-check
    pty.send(CTRL_A);
    pty.send(CTRL_F);
    pty.wait_for("find bar", |s| s.iter().any(|r| r.contains("Find")));
    assert!(pty.has_reverse_video(), "selection uses reverse video");
    pty.assert_default_background();
}

#[test]
fn find_bar_top_right_with_count_and_toggles() {
    let dir = tempfile::tempdir().unwrap();
    let mut pty = Pty::spawn(dir.path(), CFG, Some(&sample(dir.path())), &[]);
    pty.wait_for("start", |s| s.iter().any(|r| r.contains("alpha")));
    pty.send(CTRL_F);
    let s = pty.wait_for("find bar", |s| s.iter().any(|r| r.contains("Find")));
    let (row, line) = s.iter().enumerate().find(|(_, r)| r.contains("Find")).unwrap();
    let col = line.find("Find").unwrap();
    assert!(row <= 1, "top of the window (row {row})");
    assert!(col > 100 / 3, "right side of the window (col {col})");
    assert!(s.iter().any(|r| r.contains('✕') && r.find('✕').unwrap() > 80), "close button near the right edge");
    pty.send("foo");
    pty.wait_for("count", |s| s.iter().any(|r| r.contains("1 of 3")));
    pty.send(ENTER);
    pty.wait_for("next", |s| s.iter().any(|r| r.contains("2 of 3")));
    pty.send("\x1bc"); // Alt+C: match case
    pty.wait_for("case toggle", |s| s.iter().any(|r| r.contains("of 2")));
    pty.send("\x1bw"); // Alt+W: whole word
    pty.wait_for("word toggle", |s| s.iter().any(|r| r.contains("of 2")));
    pty.send(ESC);
    pty.wait_for("closed", |s| !s.iter().any(|r| r.contains("Find")));
}

#[test]
fn mouse_click_moves_the_cursor_and_double_click_selects_a_word() {
    let dir = tempfile::tempdir().unwrap();
    let mut pty = Pty::spawn(dir.path(), CFG, Some(&sample(dir.path())), &[]);
    pty.wait_for("start", |s| s.iter().any(|r| r.contains("alpha")));
    pty.click(3 + 4, 1); // line 2, column 5 (0-based 4)
    pty.wait_for("cursor moved", |s| s[28].contains("Ln 2, Col 5"));
    assert_eq!(pty.cursor(), (7, 1));
    // drag select
    pty.mouse("press", 3, 0);
    pty.mouse("drag", 8, 0);
    pty.mouse("release", 8, 0);
    pty.wait_for("selection", |s| s[28].contains("5 selected"));
    // double click
    pty.click(3 + 7, 0);
    pty.click(3 + 7, 0);
    pty.wait_for("word selected", |s| s[28].contains("4 selected"));
    // gutter click selects the line
    pty.click(1, 2);
    pty.wait_for("line selected", |s| s[28].contains("11 selected"));
}

#[test]
fn ctrl_c_ctrl_v_ctrl_a_work_in_the_terminal_in_raw_mode() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("c.txt");
    std::fs::write(&f, "hello").unwrap();
    let mut pty = Pty::spawn(dir.path(), "[editor]\nmodal = false\n[clipboard]\nbackend = \"osc52\"\n", Some(&f), &[]);
    pty.wait_for("start", |s| s.iter().any(|r| r.contains("hello")));
    pty.send(CTRL_A);
    pty.send(CTRL_C);
    pty.send("\x1b[F"); // End
    pty.send(CTRL_V);
    pty.wait_for("pasted", |s| s[0].contains("hellohello"));
    assert!(!pty.finished(), "Ctrl+C is a copy, not an interrupt");
    pty.send(CTRL_S);
    pty.wait_for("saved", |s| s[29].contains("written"));
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "hellohello");
}

#[test]
fn quit_asks_when_modified_and_exits_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("q.txt");
    std::fs::write(&f, "x").unwrap();
    let mut pty = Pty::spawn(dir.path(), "[editor]\nmodal = false\n[clipboard]\nbackend = \"osc52\"\n", Some(&f), &[]);
    pty.wait_for("start", |s| s[0].contains('x'));
    pty.send("abc");
    pty.wait_for("modified marker", |s| s[28].contains("[+]"));
    pty.send(CTRL_Q);
    pty.wait_for("prompt", |s| s[29].contains("Save changes"));
    assert!(!pty.finished());
    pty.send("n");
    assert!(pty.wait_exit(), "process exits after discarding");
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "x");
}

#[test]
fn clean_quit_with_ctrl_q_and_colon_q() {
    let dir = tempfile::tempdir().unwrap();
    let mut pty = Pty::spawn(dir.path(), CFG, None, &[]);
    pty.wait_for("start", |s| s[28].contains("NORMAL"));
    pty.send(CTRL_Q);
    assert!(pty.wait_exit());
    let mut pty = Pty::spawn(dir.path(), CFG, None, &[]);
    pty.wait_for("start", |s| s[28].contains("NORMAL"));
    pty.send(":q\r");
    assert!(pty.wait_exit());
}

#[test]
fn bad_config_warns_but_starts() {
    let dir = tempfile::tempdir().unwrap();
    let pty = Pty::spawn(dir.path(), "[editor]\nmodl = 1\n[bogus]\nx = 2\n[clipboard]\nbackend = \"osc52\"\n", None, &[]);
    pty.wait_for("warning hint", |s| s[29].contains("config warning"));
}

#[test]
fn syntax_colors_are_theme_neutral() {
    // The editor never sets a background and uses only default/ANSI-palette foregrounds, so the
    // terminal theme decides light vs dark. Verified by scanning every emulator cell.
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("a.sql");
    std::fs::write(&f, "SELECT 'x', 1 FROM t -- c\n").unwrap();
    let pty = Pty::spawn(dir.path(), CFG, Some(&f), &[]);
    pty.wait_for("start", |s| s[0].contains("SELECT"));
    pty.assert_default_background();
    pty.assert_ansi_palette_only();
}

#[test]
fn raw_terminal_output_has_no_truecolor_256_color_or_background_sequences() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("main.go");
    std::fs::write(&f, "package main\n\nfunc main() {\n\tx := []int{1, 0x1F, 2.5e-3}\n\tif x[0] == 1 && (len(x) > 2 || false) {\n\t\ts := \"str\" // comment\n\t}\n}\n").unwrap();
    let mut pty = Pty::spawn(dir.path(), CFG, Some(&f), &[]);
    pty.wait_for("start", |s| s.iter().any(|r| r.contains("package main")));
    pty.send(CTRL_A);
    pty.send(CTRL_F);
    pty.send("x");
    pty.wait_for("find bar", |s| s.iter().any(|r| r.contains("Find") || r.contains("of")));
    let raw = pty.raw_output();
    let colours = common::colorscan::sgr_lists(&raw);
    assert!(colours.iter().flatten().any(|p| (30..=37).contains(p)), "syntax colours must be present for the check to mean anything");
    let v = common::colorscan::violations(&raw);
    assert!(v.is_empty(), "{v:?}");
}
