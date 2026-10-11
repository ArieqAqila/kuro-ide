# Architecture

A new editor in Rust instead of a Neovim configuration, because several requirements (a floating find bar with
buttons and a counter, a bounded scrollable result table, a cell-buffer test layer, and an enforced docs ⇄ code
check) fit a purpose-built UI better than plugins.

| Concern | Choice | Why |
|---|---|---|
| Terminal I/O | `crossterm` | raw mode, mouse, bracketed paste, kitty keyboard flags |
| Drawing | `ratatui` buffer | UI is rendered into a cell `Buffer`, so tests assert on cells; only `Style::default()`, ANSI colours and modifiers are used, never a background |
| Text | `ropey` | rope buffer; positions are char indices; grapheme widths via `unicode-width` |
| Find | `regex` | literal/whole-word/case options compile to a regex; ranges are char ranges |
| Syntax | own table-driven tokenizer (`syntax.rs`) | five small languages; a full highlighter (tree-sitter, syntect) would add weight and RGB themes for little gain |
| Config | `serde` + `toml` | defaults in one struct; unknown keys warn |
| Clipboard | `wl-copy`/`xclip`/`xsel` subprocesses, OSC 52 fallback | no X11/Wayland client libraries needed; behind a trait so tests use a fake |
| SQL | `postgres` and `mysql` (sync) drivers on a worker thread | UI never blocks; cancel via `pg_cancel_query` / `KILL QUERY` on a second connection |

Modules: `buffer` (text, cursor, selection, undo), `find` (pure engine), `findbar` (state + geometry + drawing),
`table` (pure layout), `syntax`, `clipboard`, `config`, `sql/{profile,classify,pg,my}` and `sql` (worker/session),
`app` (state, keyboard, mouse, commands) and `render` (drawing). `main.rs` is only terminal setup and the event loop.

Data flow: terminal event → `App::handle_key/handle_mouse` → state change → `App::render(&mut Buffer)` →
ratatui diffs it onto the terminal. Query results come back from the worker through a channel and are picked up
by `App::tick()`.
