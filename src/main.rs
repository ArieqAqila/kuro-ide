use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use kuro::ansi::SgrFilter;
use kuro::app::App;
use kuro::buffer::Buffer;
use kuro::config::{Config, default_config_path};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{Stdout, stdout};
use std::path::PathBuf;
use std::time::Duration;

const HELP: &str = "kuro - a vi-style terminal editor with VS Code-like usability and a SQL client

USAGE:
    kuro [--config FILE] [FILE]

OPTIONS:
    --config FILE   read this config.toml instead of $KURO_CONFIG_DIR / ~/.config/kuro/config.toml
    --version       print the version
    -h, --help      print this help

Inside the editor: Ctrl+F find, Ctrl+H replace, Ctrl+S save, Ctrl+Q quit, :sql connect NAME
See README.md and docs/CUSTOMIZATION.md.";

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

fn setup(kitty: bool) -> Result<Term> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    if kitty {
        // terminals without the kitty keyboard protocol ignore this sequence
        let _ = execute!(stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    }
    Ok(Terminal::new(CrosstermBackend::new(SgrFilter::new(stdout())))?)
}

fn restore(kitty: bool) {
    if kitty {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableBracketedPaste, DisableMouseCapture, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

fn main() -> Result<()> {
    let Some(args) = parse_args()? else { return Ok(()) };
    let cfg_path = args.config.clone().unwrap_or_else(default_config_path);
    let (cfg, warnings) = Config::load(&cfg_path);

    let mut app = App::new(cfg.clone(), Buffer::new(), kuro::clipboard::detect(&cfg.clipboard.backend));
    if let Some(f) = &args.file {
        app.open_file(f);
    }
    app.warnings = warnings;

    let kitty = cfg.ui.kitty_keyboard;
    let mut term = setup(kitty)?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore(kitty);
        hook(info);
    }));

    let result = run(&mut term, &mut app);
    restore(kitty);
    result
}

fn run(term: &mut Term, app: &mut App) -> Result<()> {
    let mut dirty = true;
    while !app.quit {
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
        if event::poll(Duration::from_millis(50))? {
            loop {
                match event::read()? {
                    Event::Key(k) => app.handle_key(k),
                    Event::Mouse(m) => app.handle_mouse(m),
                    Event::Paste(s) => app.handle_paste(&s),
                    _ => {}
                }
                dirty = true;
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if app.tick() {
            dirty = true;
        }
    }
    Ok(())
}
