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

Input is parsed by our own state machine (`input.rs`) rather than a library, because the requirements (kitty
`CSI u`, split reads, "never a key from an incomplete sequence") must be testable in isolation. `ansi.rs` wraps the
output so that crossterm's `38;5;N` palette colours become the classic `30-37`/`90-97` codes, keeping the colour rule
(no truecolor, no 256-colour, no background) true byte-for-byte.

Modules: `buffer` (text, cursor, selection, undo), `find` (pure engine), `findbar` (state + geometry + drawing),
`table` (pure layout), `syntax`, `clipboard`, `config`, `sql/{profile,classify,pg,my}` and `sql` (worker/session),
`app` (state, keyboard, mouse, commands) and `render` (drawing). Also: `input` (terminal input parser), `indent` (tab/outdent/backspace rules),
`suggest` (word suggestions), `picker` (filterable popup), `snippets` (per-driver SQL), `keymap` (the documented bindings),
`keyspec` (configurable keys), `ansi` (colour-safe output), `sql/stmt` (statement splitting). `main.rs` is only terminal setup,
the kitty-protocol query and the event loop.

Data flow: terminal event → `App::handle_key/handle_mouse` → state change → `App::render(&mut Buffer)` →
ratatui diffs it onto the terminal. Query results come back from the worker through a channel and are picked up
by `App::tick()`.

## Extensions and colours

`src/ext/` is the extension system: `manifest` (parsing and validation), `store` (sealed install / list / remove / load, atomic registry),
`seal` (HMAC-SHA256 with a per-user key), `lang` (manifest to `LangSpec` plus the extension's rules and roles), `names` (the built-in
readers for `names_from`), `cli` (`kuro ext …`), `version` (the `kuro = ">=0.1, <0.2"` check). An extension language is an ordinary
`LangSpec` with an optional `ExtData`; the tokenizer runs the generic pass first and then `apply_ext` (regex rules, declared names, names
from the user's files), and `Kind::Ext(i)` is the language's own role `i`. `src/palette.rs` chooses extension colours from the terminal's own
palette (pure functions; the query is in `main.rs`). See [EXTENSIONS.md](EXTENSIONS.md).

## Live sync (see [SYNC.md](SYNC.md) for behaviour)

`src/sync/` is built as pure state machines with the I/O at the edge, so the hard parts can be tested without
sockets or timing:

* `ot` — operations (`Ins`, `Del`, `Mark`) and the transformation function.
* `replica` — one terminal's copy: unconfirmed edits, the stream from the leader, remote operations adjusted to the
  local unconfirmed edits.
* `leader` — the total order, shared undo/redo, atomic save with the disk check, external-change handling, the
  journal and the recovery window after a hand-over.
* `node` — a terminal as a leader-or-follower that can be promoted; carries a save across a hand-over.
* `cluster`, `sim` — a deterministic simulator (random delivery, kills, simulated lock, faulty file system) used by
  the tests; `storage` abstracts the file system and journal.
* `net`, `real` — the real thing: Unix sockets, `flock` leader lock, real files and journal. `SyncHandle` runs on the
  editor's thread (`App::sync_pump` from `tick`); helper threads only move bytes into a queue, which keeps the order
  of remote effects identical to the leader's order.

`Buffer` records every edit as operations in an outbox while synced; `App` hands them to the group before applying
anything that arrived, then applies remote operations without touching the local undo stack and moves the cursor,
selection and top line along with the text.
