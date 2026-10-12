//! What the terminal loop in `main.rs` needs from a full-screen program: the editor, and `kuro git`.

use crate::input::InputEvent;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::Rect;

pub trait Screen {
    /// Draw into `buf`; returns where to put the terminal cursor, if anywhere.
    fn render(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)>;
    fn key(&mut self, k: KeyEvent);
    fn mouse(&mut self, m: MouseEvent);
    fn paste(&mut self, s: &str);
    /// Background work; true when something on screen changed.
    fn tick(&mut self) -> bool;
    /// Keep redrawing even without input (a running query).
    fn busy(&self) -> bool;
    fn quit(&self) -> bool;
}

/// Feed parsed terminal input to a screen.
pub fn dispatch<S: Screen>(screen: &mut S, events: Vec<InputEvent>) {
    for e in events {
        match e {
            InputEvent::Key(k) => screen.key(k),
            InputEvent::Mouse(m) => screen.mouse(m),
            InputEvent::Paste(s) => screen.paste(&s),
            InputEvent::KittyKeyboard | InputEvent::DeviceAttributes | InputEvent::ColorReply(_) => {}
        }
    }
}

impl Screen for crate::app::App {
    fn render(&mut self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        crate::app::App::render(self, buf, area)
    }
    fn key(&mut self, k: KeyEvent) {
        self.handle_key(k)
    }
    fn mouse(&mut self, m: MouseEvent) {
        self.handle_mouse(m)
    }
    fn paste(&mut self, s: &str) {
        self.handle_paste(s)
    }
    fn tick(&mut self) -> bool {
        crate::app::App::tick(self)
    }
    fn busy(&self) -> bool {
        self.is_busy()
    }
    fn quit(&self) -> bool {
        self.quit
    }
}
