//! An extension in the real binary: install it with `kuro ext install`, open a file of its language, and read the colours from the
//! screen through a terminal emulator. Everything lives in a throw-away HOME.

mod common;
use common::pty::*;
use std::path::Path;
use std::process::Command;

const CFG: &str = "[editor]\nmodal = false\n[clipboard]\nbackend = \"osc52\"\n";

fn kuro_ext(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_kuro"))
        .args(args)
        .env("HOME", dir)
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .output()
        .unwrap();
    (out.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

fn install_ksystem(dir: &Path) {
    let src = format!("{}/extensions/ksystem", env!("CARGO_MANIFEST_DIR"));
    let (code, out) = kuro_ext(dir, &["ext", "install", &src]);
    assert_eq!(code, 0, "{out}");
}

fn row_of(screen: &[String], needle: &str) -> u16 {
    screen.iter().position(|r| r.contains(needle)).unwrap_or_else(|| panic!("{needle:?} not on screen:\n{}", screen.join("\n"))) as u16
}

/// The whole first frame: the text *and* the status bar (the last row drawn), so a test never looks at a half-drawn screen.
fn drawn(s: &[String], status: &str) -> bool {
    s.iter().any(|r| r.contains("PGM_FIELDS")) && s.iter().any(|r| r.contains(status))
}

#[test]
fn an_installed_extension_colours_its_files_with_palette_colours_only() {
    let dir = tempfile::tempdir().unwrap();
    install_ksystem(dir.path());
    let headers = format!("{}/tests/fixtures/ksystem/headers/*.h", env!("CARGO_MANIFEST_DIR"));
    let cfg = format!("{CFG}[ext.ksystem]\nheaders = [\"{headers}\"]\n");
    let file = dir.path().join("sct100");
    std::fs::copy(format!("{}/tests/fixtures/ksystem/screen.sct", env!("CARGO_MANIFEST_DIR")), &file).unwrap();
    let pty = Pty::spawn(dir.path(), &cfg, Some(&file), &[]);
    let s = pty.wait_for("the screen is drawn", |s| drawn(s, "ksystem"));
    assert!(s.iter().any(|r| r.contains("ksystem")), "the status bar names the language:\n{}", s.join("\n"));
    let cols = |needle: &str| s[row_of(&s, needle) as usize].find(needle).unwrap() as u16;
    let at = |needle: &str| pty.fg_at(row_of(&s, needle), cols(needle));
    // bright cyan = slot 14: locals; bright yellow = 11: calcs; bright green = 10: fields; bright magenta = 13: indexes
    assert_eq!(at("lcount  "), vt100::Color::Idx(14), "a local");
    assert_eq!(at("cready  "), vt100::Color::Idx(11), "a calc");
    let in_row = |row: &str, token: &str| {
        let r = row_of(&s, row);
        pty.fg_at(r, s[r as usize].find(token).unwrap() as u16)
    };
    assert_eq!(in_row("SCR_LOGFLD", "dmokode"), vt100::Color::Idx(10), "a file field");
    assert_eq!(in_row("SCR_JOIN_FILE1", "dmoidx"), vt100::Color::Idx(13), "an index");
    assert_eq!(in_row("SCR_JOIN_FILE1", "dmokode"), vt100::Color::Idx(10), "a file field again");
    assert_ne!(at("SCREEN 100"), vt100::Color::Default, "keywords are coloured too");
    pty.assert_default_background();
    pty.assert_ansi_palette_only();
}

#[test]
fn the_users_colour_mapping_changes_a_role_and_minus_one_is_the_default_text() {
    let dir = tempfile::tempdir().unwrap();
    install_ksystem(dir.path());
    let cfg = format!("{CFG}[ext.ksystem.colors]\nlocal = 6\ncalc = -1\n");
    let file = dir.path().join("sct1");
    std::fs::copy(format!("{}/tests/fixtures/ksystem/screen.sct", env!("CARGO_MANIFEST_DIR")), &file).unwrap();
    let pty = Pty::spawn(dir.path(), &cfg, Some(&file), &[]);
    let s = pty.wait_for("drawn", |s| drawn(s, "ksystem"));
    let at = |needle: &str| pty.fg_at(row_of(&s, needle), s[row_of(&s, needle) as usize].find(needle).unwrap() as u16);
    assert_eq!(at("lcount  "), vt100::Color::Idx(6), "slot 6 (cyan)");
    assert_eq!(at("cready  "), vt100::Color::Default, "-1: the terminal's own text colour");
    pty.assert_ansi_palette_only();
}

#[test]
fn without_the_extension_the_same_file_is_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sct100");
    std::fs::copy(format!("{}/tests/fixtures/ksystem/screen.sct", env!("CARGO_MANIFEST_DIR")), &file).unwrap();
    let pty = Pty::spawn(dir.path(), CFG, Some(&file), &[]);
    let s = pty.wait_for("drawn", |s| drawn(s, "Plain Text"));
    assert!(s.iter().any(|r| r.contains("Plain Text")));
    assert_eq!(pty.fg_at(row_of(&s, "SCREEN 100"), 4), vt100::Color::Default);
}

#[test]
fn a_tampered_extension_is_skipped_with_a_warning_and_kuro_still_starts() {
    let dir = tempfile::tempdir().unwrap();
    install_ksystem(dir.path());
    let manifest = dir.path().join("data/kuro/extensions/ksystem/kuro-ext.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, text.replace("\"BLANKS\"", "\"BLANKS\", \"EVIL\"")).unwrap();
    let file = dir.path().join("sct100");
    std::fs::copy(format!("{}/tests/fixtures/ksystem/screen.sct", env!("CARGO_MANIFEST_DIR")), &file).unwrap();
    let mut pty = Pty::spawn(dir.path(), CFG, Some(&file), &[]);
    let s = pty.wait_for("drawn", |s| drawn(s, "Plain Text"));
    assert!(s.iter().any(|r| r.contains("Plain Text")), "the extension did not load");
    pty.send("\x10"); // Ctrl+P: the command line (the editor is modeless here)
    pty.send("warnings\r");
    pty.wait_for("the warning", |s| s.iter().any(|r| r.contains("skipped") && r.contains("seal")));
    let (code, out) = kuro_ext(dir.path(), &["ext", "list"]);
    assert_eq!(code, 0);
    assert!(out.contains("NOT LOADED"), "{out}");
}

// ------------------------------------------------------------------ colours that suit the terminal (ui.adaptive_colors)

fn colour_slots(theme: Option<Theme>, extra_cfg: &str) -> [vt100::Color; 4] {
    let dir = tempfile::tempdir().unwrap();
    install_ksystem(dir.path());
    let headers = format!("{}/tests/fixtures/ksystem/headers/*.h", env!("CARGO_MANIFEST_DIR"));
    let cfg = format!("{CFG}{extra_cfg}\n[ext.ksystem]\nheaders = [\"{headers}\"]\n");
    let file = dir.path().join("sct100");
    std::fs::copy(format!("{}/tests/fixtures/ksystem/screen.sct", env!("CARGO_MANIFEST_DIR")), &file).unwrap();
    let pty = Pty::spawn_themed(dir.path(), &cfg, Some(&file), &[], Term::Kitty, theme);
    let s = pty.wait_for("drawn", |s| drawn(s, "ksystem"));
    let in_row = |row: &str, token: &str| {
        let r = row_of(&s, row);
        pty.fg_at(r, s[r as usize].find(token).unwrap() as u16)
    };
    pty.assert_default_background();
    pty.assert_ansi_palette_only();
    [in_row("lcount", "lcount"), in_row("cready", "cready"), in_row("SCR_LOGFLD", "dmokode"), in_row("SCR_JOIN_FILE1", "dmoidx")]
}

#[test]
fn on_a_dark_theme_the_declared_colours_are_kept() {
    let got = colour_slots(Some(Theme::dark()), "");
    assert_eq!(got, [vt100::Color::Idx(14), vt100::Color::Idx(11), vt100::Color::Idx(10), vt100::Color::Idx(13)]);
}

#[test]
fn on_a_light_theme_the_pale_colours_are_swapped_for_ones_that_can_be_read() {
    let got = colour_slots(Some(Theme::light()), "");
    // bright cyan, yellow, green and magenta are hard to read on white: kuro chose other slots from the terminal's own palette
    assert_ne!(got[0], vt100::Color::Idx(14));
    assert_ne!(got[1], vt100::Color::Idx(11));
    assert_ne!(got[2], vt100::Color::Idx(10));
    let distinct: std::collections::HashSet<String> = got.iter().map(|c| format!("{c:?}")).collect();
    assert_eq!(distinct.len(), 4, "still four different colours: {got:?}");
    for c in got {
        assert!(matches!(c, vt100::Color::Idx(n) if n < 16), "{c:?}: palette slots only, nothing invented");
    }
}

#[test]
fn adaptive_colours_can_be_switched_off_and_a_silent_terminal_gets_the_declared_ones() {
    let off = colour_slots(Some(Theme::light()), "[ui]\nadaptive_colors = false");
    assert_eq!(off, [vt100::Color::Idx(14), vt100::Color::Idx(11), vt100::Color::Idx(10), vt100::Color::Idx(13)]);
    let silent = colour_slots(None, "");
    assert_eq!(silent, off, "a terminal that does not answer the colour queries");
}

#[test]
fn a_role_the_user_mapped_is_never_changed_by_the_adaptation() {
    let dir = tempfile::tempdir().unwrap();
    install_ksystem(dir.path());
    let cfg = format!("{CFG}[ext.ksystem.colors]\ncalc = 11\n");
    let file = dir.path().join("sct1");
    std::fs::copy(format!("{}/tests/fixtures/ksystem/screen.sct", env!("CARGO_MANIFEST_DIR")), &file).unwrap();
    let pty = Pty::spawn_themed(dir.path(), &cfg, Some(&file), &[], Term::Kitty, Some(Theme::light()));
    let s = pty.wait_for("drawn", |s| drawn(s, "ksystem"));
    let r = row_of(&s, "cready");
    assert_eq!(pty.fg_at(r, s[r as usize].find("cready").unwrap() as u16), vt100::Color::Idx(11), "pinned, even if pale");
}
