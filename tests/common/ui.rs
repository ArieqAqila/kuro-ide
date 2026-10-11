//! Helpers for driving `App` without a terminal and inspecting the cell buffer.
#![allow(dead_code)]

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use kuro::app::App;
use kuro::buffer::Buffer;
use kuro::clipboard::MemoryClipboard;
use kuro::config::Config;
use ratatui::buffer::Buffer as Cells;
use ratatui::layout::Rect;

pub const W: u16 = 80;
pub const H: u16 = 24;

pub fn app_with(text: &str) -> (App, MemoryClipboard) {
    app_cfg(text, Config::default())
}

pub fn app_cfg(text: &str, mut cfg: Config) -> (App, MemoryClipboard) {
    // Tests never read the user's real connection profiles: point at a path that cannot exist.
    if cfg.sql.connections_file.is_empty() {
        cfg.sql.connections_file = "/nonexistent-kuro-test-dir/connections.toml".into();
    }
    let clip = MemoryClipboard::default();
    let app = App::new(cfg, Buffer::from_str(text), Box::new(clip.clone()));
    (app, clip)
}

/// A clock that only moves when told to (deterministic click timing).
#[derive(Clone)]
pub struct ManualClock {
    base: std::time::Instant,
    offset_ms: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl ManualClock {
    pub fn install(app: &mut App) -> ManualClock {
        let c = ManualClock { base: std::time::Instant::now(), offset_ms: Default::default() };
        let c2 = c.clone();
        app.set_clock(move || c2.now());
        c
    }
    pub fn now(&self) -> std::time::Instant {
        self.base + std::time::Duration::from_millis(self.offset_ms.load(std::sync::atomic::Ordering::SeqCst))
    }
    pub fn advance_ms(&self, ms: u64) {
        self.offset_ms.fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
    }
}

pub fn modeless(text: &str) -> (App, MemoryClipboard) {
    let mut cfg = Config::default();
    cfg.editor.modal = false;
    app_cfg(text, cfg)
}

pub fn key(spec: &str) -> KeyEvent {
    let mut mods = KeyModifiers::NONE;
    let mut parts: Vec<&str> = spec.split('+').collect();
    let last = parts.pop().unwrap();
    for p in parts {
        mods |= match p {
            "ctrl" => KeyModifiers::CONTROL,
            "alt" => KeyModifiers::ALT,
            "shift" => KeyModifiers::SHIFT,
            other => panic!("bad modifier {other}"),
        };
    }
    let code = match last {
        "enter" => KeyCode::Enter,
        "esc" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        "plus" => KeyCode::Char('+'),
        f if f.len() > 1 && f.starts_with('f') && f[1..].chars().all(|c| c.is_ascii_digit()) => KeyCode::F(f[1..].parse().unwrap()),
        c if c.chars().count() == 1 => KeyCode::Char(c.chars().next().unwrap()),
        other => panic!("bad key {other}"),
    };
    KeyEvent { code, modifiers: mods, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

pub fn press(app: &mut App, specs: &str) {
    for s in specs.split_whitespace() {
        app.handle_key(key(s));
    }
}

pub fn typed(app: &mut App, text: &str) {
    for c in text.chars() {
        if c == '\n' {
            app.handle_key(key("enter"));
        } else {
            app.handle_key(KeyEvent {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            });
        }
    }
}

pub fn mouse(app: &mut App, kind: MouseEventKind, x: u16, y: u16) {
    mouse_mod(app, kind, x, y, KeyModifiers::NONE)
}

pub fn mouse_mod(app: &mut App, kind: MouseEventKind, x: u16, y: u16, modifiers: KeyModifiers) {
    app.handle_mouse(MouseEvent { kind, column: x, row: y, modifiers });
}

pub fn click(app: &mut App, x: u16, y: u16) {
    mouse(app, MouseEventKind::Down(MouseButton::Left), x, y);
    mouse(app, MouseEventKind::Up(MouseButton::Left), x, y);
}

pub fn render(app: &mut App) -> Cells {
    render_size(app, W, H)
}

pub fn render_size(app: &mut App, w: u16, h: u16) -> Cells {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Cells::empty(area);
    app.render(&mut buf, area);
    buf
}

pub fn rows(buf: &Cells) -> Vec<String> {
    kuro::render::rows_text(buf)
}

pub fn screen(app: &mut App) -> Vec<String> {
    rows(&render(app))
}

pub fn text_of(app: &App) -> String {
    app.buffer.text()
}

/// (x, y) of the first occurrence of `needle` in the buffer.
pub fn find_on(buf: &Cells, needle: &str) -> Option<(u16, u16)> {
    for (y, row) in rows(buf).iter().enumerate() {
        if let Some(i) = row.find(needle) {
            return Some((row[..i].chars().count() as u16, y as u16));
        }
    }
    None
}

/// Width of the line-number gutter (also the x of the first text column).
pub fn gutter_w(app: &App) -> u16 {
    app.layout(Rect::new(0, 0, W, H)).gutter_w
}
