//! Drive the real `kuro` binary in a pseudo-terminal and read the screen through a terminal emulator (vt100).
#![allow(dead_code)]

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How the emulated terminal answers kuro's startup queries.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Term {
    /// Answers the kitty keyboard query and device attributes (like kitty, foot, WezTerm).
    Kitty,
    /// Answers device attributes only: a terminal without the kitty protocol.
    Legacy,
    /// Answers nothing: exercises kuro's detection timeout.
    Silent,
}

/// The colours an emulated terminal reports when asked with `OSC 4` / `OSC 10` / `OSC 11`.
#[derive(Clone, Copy)]
pub struct Theme {
    pub slots: [(u8, u8, u8); 16],
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
}

impl Theme {
    const XTERM: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    pub fn dark() -> Theme {
        Theme { slots: Self::XTERM, fg: (229, 229, 229), bg: (0, 0, 0) }
    }
    pub fn light() -> Theme {
        Theme { slots: Self::XTERM, fg: (30, 30, 30), bg: (255, 255, 255) }
    }
}

fn osc_reply(code: &str, c: (u8, u8, u8)) -> Vec<u8> {
    let w = |v: u8| format!("{:04x}", v as u32 * 257);
    format!("\x1b]{code};rgb:{}/{}/{}\x1b\\", w(c.0), w(c.1), w(c.2)).into_bytes()
}

pub struct Pty {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    raw: Arc<Mutex<Vec<u8>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    pub rows: u16,
    pub cols: u16,
}

/// Esc as a kitty-protocol terminal sends it (`CSI 27 u`): unambiguous, so it can be followed immediately by other keys.
pub const ESC: &str = "\x1b[27u";
/// A bare ESC byte, as legacy terminals send it: only meaningful after a quiet period.
pub const ESC_LEGACY: &str = "\x1b";
/// Ctrl+Enter under the kitty keyboard protocol.
pub const CTRL_ENTER: &str = "\x1b[13;5u";
/// Alt+Enter (the default fallback for terminals without Ctrl+Enter).
pub const ALT_ENTER: &str = "\x1b\r";
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
        Pty::spawn_term(dir, config, file, env, Term::Kitty)
    }

    pub fn spawn_term(dir: &Path, config: &str, file: Option<&Path>, env: &[(&str, &str)], term: Term) -> Pty {
        Pty::spawn_themed(dir, config, file, env, term, None)
    }

    /// Like `spawn_term`, and the emulated terminal answers colour queries with `theme` (it stays silent about colours when `None`).
    pub fn spawn_themed(dir: &Path, config: &str, file: Option<&Path>, env: &[(&str, &str)], term: Term, theme: Option<Theme>) -> Pty {
        let mut args = vec!["--config".to_string(), dir.join("config.toml").display().to_string()];
        if let Some(f) = file {
            args.push(f.display().to_string());
        }
        Pty::spawn_general(dir, config, &args, None, env, term, theme)
    }

    /// `kuro <args>` in `cwd`, with `config` as the config file found through KURO_CONFIG_DIR (for sub-commands such as `kuro git`).
    pub fn spawn_args(dir: &Path, config: &str, args: &[&str], cwd: &Path, env: &[(&str, &str)]) -> Pty {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        Pty::spawn_general(dir, config, &args, Some(cwd), env, Term::Kitty, None)
    }

    fn spawn_general(
        dir: &Path,
        config: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(&str, &str)],
        term: Term,
        theme: Option<Theme>,
    ) -> Pty {
        let (rows, cols) = (30, 100);
        std::fs::write(dir.join("config.toml"), config).unwrap();
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_kuro"));
        for a in args {
            cmd.arg(a);
        }
        if let Some(c) = cwd {
            cmd.cwd(c);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("KURO_CONFIG_DIR", dir);
        // never touch the real clipboard from tests
        // nor the real sync sockets or journals
        cmd.env("XDG_RUNTIME_DIR", dir.join("run"));
        cmd.env("XDG_STATE_HOME", dir.join("state"));
        cmd.env("XDG_DATA_HOME", dir.join("data"));
        cmd.env_remove("WAYLAND_DISPLAY");
        cmd.env_remove("DISPLAY");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let reply = writer.clone();
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let p2 = parser.clone();
        let raw = Arc::new(Mutex::new(Vec::new()));
        let raw2 = raw.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                raw2.lock().unwrap().extend_from_slice(&buf[..n]);
                // answer startup queries like a terminal would
                let chunk = &buf[..n];
                let has = |pat: &[u8]| chunk.windows(pat.len()).any(|w| w == pat);
                let mut answer = Vec::new();
                if term == Term::Kitty && has(b"\x1b[?u") {
                    answer.extend_from_slice(b"\x1b[?0u");
                }
                if let Some(t) = theme {
                    let text = String::from_utf8_lossy(chunk);
                    if text.contains("\x1b]10;?") {
                        answer.extend(osc_reply("10", t.fg));
                    }
                    if text.contains("\x1b]11;?") {
                        answer.extend(osc_reply("11", t.bg));
                    }
                    for n in 0..16usize {
                        if text.contains(&format!("\x1b]4;{n};?")) {
                            answer.extend(osc_reply(&format!("4;{n}"), t.slots[n]));
                        }
                    }
                }
                if term != Term::Silent && has(b"\x1b[c") {
                    answer.extend_from_slice(b"\x1b[?62;c");
                }
                if !answer.is_empty() {
                    let mut w = reply.lock().unwrap();
                    let _ = w.write_all(&answer);
                    let _ = w.flush();
                }
                p2.lock().unwrap().process(&buf[..n]);
            }
        });
        // keep the master alive for the lifetime of the child
        std::mem::forget(pair.master);
        Pty { writer, parser, raw, child, rows, cols }
    }

    /// Every byte the program has written to the terminal so far.
    pub fn raw_output(&self) -> Vec<u8> {
        self.raw.lock().unwrap().clone()
    }

    pub fn send(&mut self, s: &str) {
        let mut w = self.writer.lock().unwrap();
        w.write_all(s.as_bytes()).unwrap();
        w.flush().unwrap();
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

    /// The foreground colour of the cell at (row, col) as the emulator sees it.
    pub fn fg_at(&self, row: u16, col: u16) -> vt100::Color {
        let p = self.parser.lock().unwrap();
        p.screen().cell(row, col).map(|c| c.fgcolor()).unwrap_or(vt100::Color::Default)
    }

    pub fn has_reverse_video(&self) -> bool {
        let p = self.parser.lock().unwrap();
        let s = p.screen();
        (0..self.rows).any(|r| (0..self.cols).any(|c| s.cell(r, c).is_some_and(|x| x.inverse())))
    }

    /// Kill the process the way the OS would on a crash: no cleanup of any kind runs.
    pub fn sigkill(&mut self) {
        if let Some(pid) = self.child.process_id() {
            // SAFETY: plain kill(2) on a child process we spawned.
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
        assert!(self.wait_exit(), "the killed process is gone");
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
