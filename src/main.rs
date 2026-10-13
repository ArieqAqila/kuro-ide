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
    kuro ext install DIR | remove NAME | list     manage extensions (see docs/EXTENSIONS.md)
    kuro history list | show ID | delete ID… | clear   manage the hard-undo history (docs/HARD-UNDO.md)
    kuro git [-fl FILE]                            browse the history of the git repository you are in (docs/GIT.md)

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
                println!("kuro {} ({})", env!("CARGO_PKG_VERSION"), env!("KURO_GIT"));
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

/// Ask the terminal for its colours (`OSC 4` for slots 1-15, `OSC 10` text, `OSC 11` background), ended by a device-attributes query that
/// every terminal answers, so a terminal that ignores the colour queries costs one quick round trip. Bounded wait.
fn query_palette(rx: &Receiver<Vec<u8>>, parser: &mut InputParser) -> (kuro::palette::Palette, Vec<InputEvent>) {
    let mut q = String::from("\x1b]10;?\x07\x1b]11;?\x07");
    for n in 0..16 {
        q.push_str(&format!("\x1b]4;{n};?\x07"));
    }
    q.push_str("\x1b[c");
    let mut out = stdout();
    let _ = out.write_all(q.as_bytes());
    let _ = out.flush();
    let deadline = Instant::now() + Duration::from_millis(400);
    let (mut pal, mut typed) = (kuro::palette::Palette::default(), vec![]);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left.min(Duration::from_millis(50))) {
            Ok(bytes) => {
                for e in parser.feed(&bytes) {
                    match e {
                        InputEvent::ColorReply(p) => pal.feed_osc(&p),
                        InputEvent::DeviceAttributes => return (pal, typed),
                        InputEvent::KittyKeyboard => {}
                        other => typed.push(other),
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    (pal, typed)
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
                        InputEvent::ColorReply(_) => {}
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

const GIT_HELP: &str = "kuro git - browse the history of the git repository you are in

USAGE:
    kuro git                 the file tree, with each file's history, and the history of the repository
    kuro git -fl FILE        open the commit history of one file directly (renames are followed)
    kuro git --help

Inside: Enter opens, Esc goes back, q quits, o opens a file as it was at a commit (read-only). See docs/GIT.md.";

/// `kuro git [-fl FILE]`
fn git_main(args: &[String]) -> Result<()> {
    use kuro::gitview::GitApp;
    use kuro::gitview::cmd::{Git, GitError};
    let mut file: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{GIT_HELP}");
                return Ok(());
            }
            "-fl" | "--file-log" => match it.next() {
                Some(f) => file = Some(f.clone()),
                None => {
                    eprintln!("-fl needs a file name\n{GIT_HELP}");
                    std::process::exit(2);
                }
            },
            other => {
                eprintln!("unknown argument {other}\n{GIT_HELP}");
                std::process::exit(2);
            }
        }
    }
    let cwd = std::env::current_dir().context("cannot read the current directory")?;
    let git = match Git::open(&cwd, vec![]) {
        Ok(g) => g,
        Err(GitError::NotRepo) => {
            eprintln!("this directory is not in a git repository");
            std::process::exit(128);
        }
        Err(GitError::NoGit) => {
            eprintln!("git is not installed");
            std::process::exit(127);
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let rel = match &file {
        Some(f) => match git.repo_path(&cwd, f) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        None => None,
    };
    let (cfg, _warnings) = Config::load(&default_config_path());
    let mut app = match GitApp::new(git, cfg.clone(), rel) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let (loaded, _) = kuro::ext::store::load_all(&kuro::ext::store::ExtPaths::from_env(), env!("CARGO_PKG_VERSION"));
    app.ext_langs = loaded.iter().flat_map(|e| kuro::ext::lang::build(&e.manifest)).collect();
    let mut term = setup()?;
    let rx = spawn_reader();
    let mut parser = InputParser::new();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore(false);
        hook(info);
    }));
    let result = run(&mut term, &mut app, &rx, &mut parser, vec![]);
    restore(false);
    result
}

fn main() -> Result<()> {
    // `kuro ext ...` is a plain command: no terminal, no editor
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("ext") {
        let out = kuro::ext::cli::run(&argv[1..], &kuro::ext::store::ExtPaths::from_env(), env!("CARGO_PKG_VERSION"));
        print!("{}", out.stdout);
        eprint!("{}", out.stderr);
        std::process::exit(out.code);
    }
    if argv.first().map(String::as_str) == Some("history") {
        let (cfg, _) = Config::load(&default_config_path());
        let store = kuro::history::Store::new(kuro::history::Store::default_root(), cfg.history.options());
        let ask = |n: usize, size: u64| -> bool {
            use std::io::IsTerminal;
            if !std::io::stdin().is_terminal() {
                return false;
            }
            eprint!("delete the history of {n} file(s) ({size} bytes)? this cannot be undone [yes/N] ");
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).is_ok() && line.trim() == "yes"
        };
        let out = kuro::history::cli::run(&argv[1..], &store, Some(&ask));
        print!("{}", out.stdout);
        eprint!("{}", out.stderr);
        std::process::exit(out.code);
    }
    if argv.first().map(String::as_str) == Some("git") {
        return git_main(&argv[1..]);
    }
    let Some(args) = parse_args()? else { return Ok(()) };
    let cfg_path = args.config.clone().unwrap_or_else(default_config_path);
    let (cfg, warnings) = Config::load(&cfg_path);

    let mut app = App::new(cfg.clone(), Buffer::new(), kuro::clipboard::detect(&cfg.clipboard.backend));
    app.warnings = warnings;
    // installed extensions (a failed one is a warning, never a failure to start)
    let (loaded, ext_warnings) = kuro::ext::store::load_all(&kuro::ext::store::ExtPaths::from_env(), env!("CARGO_PKG_VERSION"));
    app.warnings.extend(ext_warnings);
    app.set_ext_langs(loaded.iter().flat_map(|e| kuro::ext::lang::build(&e.manifest)).collect());
    // the hard-undo history: every write kuro makes is recorded here; it only ever tells you when it grows large
    if cfg.history.enabled {
        let store = std::sync::Arc::new(kuro::history::Store::new(kuro::history::Store::default_root(), cfg.history.options()));
        let (size, warn) = (store.size_bytes(), store.opts.warn_bytes);
        if warn > 0 && size > warn {
            app.flash(format!("hard-undo history uses {} MiB: kuro history list (nothing is deleted automatically)", size >> 20));
        }
        app.set_history(store);
    }

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
    // extension colours that suit the terminal: only asked when an extension is installed
    let mut early = early;
    if cfg.ui.adaptive_colors && !app.ext_langs.is_empty() {
        let (palette, typed) = query_palette(&rx, &mut parser);
        early.extend(typed);
        app.set_palette(palette);
    }
    if cfg.sync.enabled {
        app.set_sync_options(kuro::sync::net::SyncOptions::from_env(cfg.sync.max_file_mib.saturating_mul(1024 * 1024)));
    }
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

use kuro::screen::{Screen, dispatch};

fn run<S: Screen>(term: &mut Term, app: &mut S, rx: &Receiver<Vec<u8>>, parser: &mut InputParser, early: Vec<InputEvent>) -> Result<()> {
    let mut dirty = true;
    let mut size = term.size()?;
    dispatch(app, early);
    while !app.quit() {
        let now = term.size()?;
        if now != size {
            size = now;
            dirty = true;
        }
        if dirty || app.busy() {
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
