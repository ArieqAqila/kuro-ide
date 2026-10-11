//! Color rule (Part 1.1): text colours come only from the terminal palette (default fg, ANSI 0-15);
//! no truecolor, no 256-colour, no background colour sequences are ever emitted.
//! `49`/`39`/`59` (reset to default) are allowed: they select the terminal's own colours.

mod common;
use common::sqlview::*;
use common::ui::*;
use kuro::app::App;
use kuro::config::Config;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};

use common::colorscan::{sgr_lists, violations};

// ---- the scanner itself must detect violations -------------------------------------------

#[test]
fn scanner_detects_truecolor_256_and_background_sequences() {
    assert!(!violations(b"\x1b[38;2;10;20;30mx").is_empty(), "truecolor fg");
    assert!(!violations(b"\x1b[48;2;1;2;3mx").is_empty(), "truecolor bg");
    assert!(!violations(b"\x1b[38;5;200mx").is_empty(), "256-colour fg");
    assert!(!violations(b"\x1b[48;5;17mx").is_empty(), "256-colour bg");
    assert!(!violations(b"\x1b[44mx").is_empty(), "ANSI background");
    assert!(!violations(b"\x1b[103mx").is_empty(), "bright ANSI background");
    assert!(!violations(b"\x1b[1;38:2::1:2:3mx").is_empty(), "colon form");
    assert!(!violations(b"\x1b[58;5;1mx").is_empty(), "underline colour");
}

#[test]
fn scanner_accepts_palette_colours_modifiers_and_default_resets() {
    let ok = b"\x1b[31mred\x1b[92mbright\x1b[39m\x1b[49m\x1b[59m\x1b[0m\x1b[1;4;7mx\x1b[2;3m\x1b[m";
    assert_eq!(violations(ok), Vec::<String>::new());
    assert!(!sgr_lists(ok).is_empty());
}

// ---- rendering through the real ratatui/crossterm backend ----------------------------------

/// A writer whose bytes stay readable after ratatui takes ownership of it.
#[derive(Clone, Default)]
struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn emitted(app: &mut App, w: u16, h: u16) -> Vec<u8> {
    let area = Rect::new(0, 0, w, h);
    let sink = Sink::default();
    // the same wrapping main() uses
    let backend = CrosstermBackend::new(kuro::ansi::SgrFilter::new(sink.clone()));
    let mut term = Terminal::with_options(backend, TerminalOptions { viewport: Viewport::Fixed(area) }).unwrap();
    term.draw(|f| {
        app.render(f.buffer_mut(), area);
    })
    .unwrap();
    sink.0.lock().unwrap().clone()
}

fn assert_clean(app: &mut App, what: &str) {
    let bytes = emitted(app, 100, 30);
    assert!(!bytes.is_empty());
    let v = violations(&bytes);
    assert!(v.is_empty(), "{what}: {v:?}");
}

const CODE: &str = "package main\n\nfunc main() {\n\tx := []int{1, 0x1F, 2.5e-3}\n\tif x[0] == 1 && (len(x) > 2 || false) {\n\t\ts := \"str\" // comment\n\t\t_ = s\n\t}\n}\n";

#[test]
fn syntax_coloured_code_emits_only_palette_colours() {
    let (mut app, _) = app_with(CODE);
    app.lang = &kuro::syntax::GO;
    let bytes = emitted(&mut app, 100, 30);
    assert!(!sgr_lists(&bytes).iter().all(|p| p == &[0]), "the test must see colours");
    assert!(violations(&bytes).is_empty(), "{:?}", violations(&bytes));
    // and it really contains foreground palette colours (so the check is not vacuous)
    assert!(sgr_lists(&bytes).iter().flatten().any(|p| (30..=37).contains(p) || (90..=97).contains(p)));
}

#[test]
fn every_ui_element_emits_only_palette_colours() {
    let (mut app, _) = app_with(CODE);
    app.lang = &kuro::syntax::GO;
    assert_clean(&mut app, "editor");
    press(&mut app, "ctrl+a");
    assert_clean(&mut app, "selection");
    press(&mut app, "ctrl+h");
    typed(&mut app, "x");
    assert_clean(&mut app, "find bar with matches and replace row");
    press(&mut app, "esc");
    show_rows(&mut app, 30, 6);
    assert_clean(&mut app, "result table");
    press(&mut app, "f4");
    assert_clean(&mut app, "focused result table");
    show_error(&mut app, "syntax", "boom", Some(1), Some(2));
    assert_clean(&mut app, "error pane");
    press(&mut app, "esc");
    press(&mut app, ":");
    assert_clean(&mut app, "command line");
    press(&mut app, "esc");
    press(&mut app, "alt+s");
    assert_clean(&mut app, "snippet picker");
    press(&mut app, "esc");
}

#[test]
fn suggestion_popup_emits_only_palette_colours() {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    let (mut app, _) = app_cfg("hello help helmet\n", cfg);
    app.buffer.cursor = app.buffer.len_chars();
    typed(&mut app, "hel");
    assert!(app.suggest.is_some());
    assert_clean(&mut app, "suggestion popup");
}

#[test]
fn custom_slots_including_bright_ones_stay_in_the_palette() {
    let mut cfg = Config::default();
    cfg.colors.keyword = 12;
    cfg.colors.string = 10;
    cfg.colors.bracket_depth = vec![9, 11, 13, 14, 15, 0, 7, 8];
    let (mut app, _) = app_cfg(CODE, cfg);
    app.lang = &kuro::syntax::GO;
    let bytes = emitted(&mut app, 100, 30);
    assert!(violations(&bytes).is_empty(), "{:?}", violations(&bytes));
    assert!(sgr_lists(&bytes).iter().flatten().any(|p| (90..=97).contains(p)), "bright slots use 90-97");
}

#[test]
fn ansi_slot_mapping_is_exactly_the_16_palette_colours() {
    use kuro::render::ansi_color;
    use ratatui::style::Color;
    for slot in 0..=15i8 {
        let c = ansi_color(slot).unwrap();
        assert!(!matches!(c, Color::Rgb(..) | Color::Indexed(_) | Color::Reset), "slot {slot} -> {c:?}");
    }
    assert_eq!(ansi_color(-1), None, "-1 means the default foreground");
    assert_eq!(ansi_color(16), None);
    assert_eq!(ansi_color(1), Some(Color::Red));
    assert_eq!(ansi_color(9), Some(Color::LightRed));
}

#[test]
fn bracket_depth_colours_cycle_through_the_configured_slots() {
    use kuro::render::ansi_color;
    let mut cfg = Config::default();
    cfg.colors.bracket_depth = vec![1, 2, 3];
    let (mut app, _) = app_cfg("((((x))))", cfg);
    app.lang = &kuro::syntax::GO;
    let buf = render(&mut app);
    let gw = gutter_w(&app);
    let fg = |i: u16| buf[(gw + i, 0)].fg;
    let want = |slot: i8| ansi_color(slot).unwrap();
    assert_eq!([fg(0), fg(1), fg(2), fg(3)], [want(1), want(2), want(3), want(1)], "depth 3 wraps to the first colour");
    assert_eq!([fg(5), fg(6), fg(7), fg(8)], [want(1), want(3), want(2), want(1)], "closers match their openers");
}

#[test]
fn mismatched_brackets_are_drawn_in_reverse_video() {
    use ratatui::style::Modifier;
    let (mut app, _) = app_with("f(a]");
    app.lang = &kuro::syntax::GO;
    let buf = render(&mut app);
    let gw = gutter_w(&app);
    assert!(buf[(gw + 3, 0)].modifier.contains(Modifier::REVERSED), "the stray ] is flagged");
    assert!(buf[(gw + 1, 0)].modifier.contains(Modifier::REVERSED), "and the opener left open");
    assert!(!buf[(gw, 0)].modifier.contains(Modifier::REVERSED), "ordinary text is not");
}

#[test]
fn colour_roles_follow_the_config() {
    use kuro::render::ansi_color;
    let mut cfg = Config::default();
    cfg.colors.keyword = 13;
    let (mut app, _) = app_cfg("func x() {}", cfg);
    app.lang = &kuro::syntax::GO;
    let buf = render(&mut app);
    assert_eq!(buf[(gutter_w(&app), 0)].fg, ansi_color(13).unwrap());
}
