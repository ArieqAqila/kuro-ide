//! Drive the real `kuro` binary in a pseudo-terminal and read the screen through a terminal emulator (vt100).
#![allow(dead_code)]

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Pty {
    writer: Box<dyn Write + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    pub rows: u16,
    pub cols: u16,
}

pub const ESC: &str = "\x1b";
pub const CTRL_A: &str = "\x01";
pub const CTRL_C: &str = "\x03";
pub const CTRL_F: &str = "\x06";
pub const CTRL_Q: &str = "\x11";
pub const CTRL_S: &str = "\x13";
pub const CTRL_V: &str = "\x16";
pub const ENTER: &str = "\r";
pub const F4: &str = "\x1bOS";
pub const F6: &str = "\x1b[17~";
pub const DOWN: &str = "\x1b[B";
pub const RIGHT: &str = "\x1b[C";
pub const PGDN: &str = "\x1b[6~";

impl Pty {
    /// `config` is written to <dir>/config.toml; extra env vars are passed through.
    pub fn spawn(dir: &Path, config: &str, file: Option<&Path>, env: &[(&str, &str)]) -> Pty {
        let (rows, cols) = (30, 100);
        std::fs::write(dir.join("config.toml"), config).unwrap();
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_kuro"));
        cmd.arg("--config");
        cmd.arg(dir.join("config.toml"));
        if let Some(f) = file {
            cmd.arg(f);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("KURO_CONFIG_DIR", dir);
        // never touch the real clipboard from tests
        cmd.env_remove("WAYLAND_DISPLAY");
        cmd.env_remove("DISPLAY");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let p2 = parser.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                p2.lock().unwrap().process(&buf[..n]);
            }
        });
        // keep the master alive for the lifetime of the child
        std::mem::forget(pair.master);
        Pty { writer, parser, child, rows, cols }
    }

    pub fn send(&mut self, s: &str) {
        self.writer.write_all(s.as_bytes()).unwrap();
        self.writer.flush().unwrap();
        std::thread::sleep(Duration::from_millis(40));
    }

    pub fn mouse(&mut self, kind: &str, x: u16, y: u16) {
        // SGR mouse: kind is the button code (0 = left press, 32 = left drag, 64 = wheel up, 65 = wheel down)
        let (code, end) = match kind {
            "press" => (0, 'M'),
            "release" => (0, 'm'),
            "drag" => (32, 'M'),
            "wheel_down" => (65, 'M'),
            "wheel_up" => (64, 'M'),
            other => panic!("{other}"),
        };
        self.send(&format!("\x1b[<{code};{};{}{end}", x + 1, y + 1));
    }

    pub fn click(&mut self, x: u16, y: u16) {
        self.mouse("press", x, y);
        self.mouse("release", x, y);
    }

    pub fn screen(&self) -> Vec<String> {
        let p = self.parser.lock().unwrap();
        p.screen().rows(0, self.cols).collect()
    }

    pub fn cursor(&self) -> (u16, u16) {
        let p = self.parser.lock().unwrap();
        let (r, c) = p.screen().cursor_position();
        (c, r)
    }

    pub fn wait_for(&self, what: &str, f: impl Fn(&[String]) -> bool) -> Vec<String> {
        self.wait_for_secs(what, 8, f)
    }

    pub fn wait_for_secs(&self, what: &str, secs: u64, f: impl Fn(&[String]) -> bool) -> Vec<String> {
        let t = Instant::now();
        loop {
            let s = self.screen();
            if f(&s) {
                return s;
            }
            if t.elapsed() > Duration::from_secs(secs) {
                panic!("timed out waiting for {what}; screen was:\n{}", s.join("\n"));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Every cell must use the terminal's default background.
    pub fn assert_default_background(&self) {
        let p = self.parser.lock().unwrap();
        let s = p.screen();
        for r in 0..self.rows {
            for c in 0..self.cols {
                if let Some(cell) = s.cell(r, c) {
                    assert_eq!(cell.bgcolor(), vt100::Color::Default, "cell ({c},{r}) {:?} has a background color", cell.contents());
                }
            }
        }
    }

    /// Foregrounds must be the default or one of the 16 ANSI palette colors (so light/dark themes decide).
    pub fn assert_ansi_palette_only(&self) {
        let p = self.parser.lock().unwrap();
        let s = p.screen();
        for r in 0..self.rows {
            for c in 0..self.cols {
                if let Some(cell) = s.cell(r, c) {
                    match cell.fgcolor() {
                        vt100::Color::Default => {}
                        vt100::Color::Idx(n) if n < 16 => {}
                        other => panic!("cell ({c},{r}) {:?} uses non-palette color {other:?}", cell.contents()),
                    }
                }
            }
        }
    }

    pub fn has_reverse_video(&self) -> bool {
        let p = self.parser.lock().unwrap();
        let s = p.screen();
        (0..self.rows).any(|r| (0..self.cols).any(|c| s.cell(r, c).is_some_and(|x| x.inverse())))
    }

    pub fn finished(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    pub fn wait_exit(&mut self) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(5) {
            if self.finished() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
