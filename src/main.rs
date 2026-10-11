use anyhow::{Context, Result};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use kuro::ansi::SgrFilter;
use kuro::app::App;
use kuro::buffer::Buffer;
use kuro::config::{Config, default_config_path};
use kuro::input::{InputEvent, InputParser};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{Read, Stdout, Write, stdout};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const HELP: &str = "kuro - a vi-style terminal editor with VS Code-like usability and a SQL client

USAGE:
    kuro [--config FILE] [FILE]

OPTIONS:
    --config FILE   read this config.toml instead of $KURO_CONFIG_DIR / ~/.config/kuro/config.toml
    --version       print the version
    -h, --help      print this help

Inside the editor: Ctrl+F find, Ctrl+H replace, Ctrl+S save, Ctrl+Q quit, :connect NAME, Ctrl+Enter runs SQL
See README.md, docs/KEYBINDINGS.md and docs/CUSTOMIZATION.md.";

struct Args {
    config: Option<PathBuf>,
    file: Option<PathBuf>,
}

fn parse_args() -> Result<Option<Args>> {
    let mut a = Args { config: None, file: None };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("kuro {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(None);
            }
            "--config" => a.config = Some(it.next().context("--config needs a path")?.into()),
            s if s.starts_with('-') && s != "-" => anyhow::bail!("unknown option {s} (try --help)"),
            s => a.file = Some(s.into()),
        }
    }
    Ok(Some(a))
}

type Term = Terminal<CrosstermBackend<SgrFilter<Stdout>>>;

fn setup() -> Result<Term> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    Ok(Terminal::new(CrosstermBackend::new(SgrFilter::new(stdout())))?)
}

fn restore(kitty: bool) {
    if kitty {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableBracketedPaste, DisableMouseCapture, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

/// Raw bytes from the terminal, read on a helper thread so the main loop can time out.
fn spawn_reader() -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 4096];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

/// Ask the terminal whether it speaks the kitty keyboard protocol: send `CSI ? u` (kitty query) followed by
/// `CSI c` (primary device attributes, which every terminal answers). If the DA reply arrives without a kitty
/// reply before it, the protocol is not supported. Gives up after a short, bounded wait.
/// Returns (supported, events typed meanwhile).
fn detect_kitty(rx: &Receiver<Vec<u8>>, parser: &mut InputParser) -> (bool, Vec<InputEvent>) {
    let mut out = stdout();
    let _ = out.write_all(b"\x1b[?u\x1b[c");
    let _ = out.flush();
    let deadline = Instant::now() + Duration::from_millis(500);
    let (mut kitty, mut typed) = (false, vec![]);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left.min(Duration::from_millis(50))) {
            Ok(bytes) => {
                for e in parser.feed(&bytes) {
                    match e {
                        InputEvent::KittyKeyboard => kitty = true,
                        InputEvent::DeviceAttributes => return (kitty, typed),
                        other => typed.push(other),
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    (kitty, typed)
}

fn main() -> Result<()> {
    let Some(args) = parse_args()? else { return Ok(()) };
    let cfg_path = args.config.clone().unwrap_or_else(default_config_path);
    let (cfg, warnings) = Config::load(&cfg_path);

    let mut app = App::new(cfg.clone(), Buffer::new(), kuro::clipboard::detect(&cfg.clipboard.backend));
    app.warnings = warnings;

    let mut term = setup()?;
    let rx = spawn_reader();
    let mut parser = InputParser::new();

    // keyboard protocol: auto = ask the terminal, on = trust, off = never
    let (kitty, early) = match cfg.ui.kitty_keyboard.as_str() {
        "off" => (false, vec![]),
        "on" => (true, vec![]),
        _ => detect_kitty(&rx, &mut parser),
    };
    if kitty {
        // terminals without the protocol never see this; those with it now report Ctrl+Enter, Shift+Tab, Esc unambiguously
        let _ = execute!(stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    }
    app.set_kitty(kitty);
    if let Some(f) = &args.file {
        app.open_file(f);
    }

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore(kitty);
        hook(info);
    }));

    let result = run(&mut term, &mut app, &rx, &mut parser, early);
    restore(kitty);
    result
}

fn dispatch(app: &mut App, events: Vec<InputEvent>) {
    for e in events {
        match e {
            InputEvent::Key(k) => app.handle_key(k),
            InputEvent::Mouse(m) => app.handle_mouse(m),
            InputEvent::Paste(s) => app.handle_paste(&s),
            InputEvent::KittyKeyboard | InputEvent::DeviceAttributes => {}
        }
    }
}

fn run(term: &mut Term, app: &mut App, rx: &Receiver<Vec<u8>>, parser: &mut InputParser, early: Vec<InputEvent>) -> Result<()> {
    let mut dirty = true;
    let mut size = term.size()?;
    dispatch(app, early);
    while !app.quit {
        let now = term.size()?;
        if now != size {
            size = now;
            dirty = true;
        }
        if dirty || app.is_busy() {
            term.draw(|f| {
                let area = f.area();
                let cursor = app.render(f.buffer_mut(), area);
                if let Some((x, y)) = cursor {
                    f.set_cursor_position((x, y));
                }
            })?;
            dirty = false;
        }
        // a pending lone ESC is resolved after a short quiet period; otherwise wait a little for input
        let wait = if parser.has_pending() { Duration::from_millis(30) } else { Duration::from_millis(50) };
        match rx.recv_timeout(wait) {
            Ok(bytes) => {
                let mut events = parser.feed(&bytes);
                while let Ok(more) = rx.try_recv() {
                    events.extend(parser.feed(&more));
                }
                if !events.is_empty() {
                    dirty = true;
                }
                dispatch(app, events);
            }
            Err(RecvTimeoutError::Timeout) => {
                let events = parser.flush_timeout();
                if !events.is_empty() {
                    dirty = true;
                    dispatch(app, events);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if app.tick() {
            dirty = true;
        }
    }
    Ok(())
}
