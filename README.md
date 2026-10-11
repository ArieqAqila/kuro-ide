# kuro

A `vi`-style terminal editor that behaves like VS Code in usability, with a built-in SQL client
for **PostgreSQL** and **MariaDB**. It draws with your terminal's own background and ANSI palette:
it never sets a background colour, so dark and light themes (and transparency) just work.

* modal editing (Normal / Insert / Visual) that can be switched off for a VS Code feel
* mouse everywhere: click, drag, double-click word, triple-click line, gutter click, wheel
* system clipboard: Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+A (`wl-clipboard`, `xclip`, `xsel`, or OSC 52)
* VS Code-style find bar (top-right) with `3 of 12`, Match case / Whole word / Regex, and replace
* line-number gutter with the current line highlighted and relative numbers (on by default)
* syntax highlighting: SQL, Go, Python, shell, JSON (table-driven, easy to extend)
* `:sql` client: run a selection or the whole buffer, scrollable result table, timing, row count,
  classified errors (syntax / runtime / execution) with line and column, cancel a running query

## Requirements

* Linux/macOS terminal that speaks xterm sequences (tested in `foot`; nothing is terminal-specific)
* Rust 1.85+ (edition 2024) to build: `pacman -S rust` on Arch
* Optional: `wl-clipboard` (Wayland) or `xclip` / `xsel` (X11) for the system clipboard.
  Without them, OSC 52 is used for copy.
* Optional: Docker, only to run the integration tests

## Install (fish)

```fish
git clone <this repo> ~/src/kuro; and cd ~/src/kuro
make install                       # cargo install --path . + fish completions
fish_add_path ~/.cargo/bin         # once, if ~/.cargo/bin is not on your PATH yet
kuro --version
```

Use it as `vi` **without touching the system `vi`** (see [docs/ENVIRONMENT.md](docs/ENVIRONMENT.md) for what
`vi` resolves to on this machine):

```fish
alias vi kuro; funcsave vi         # switch: `vi` now runs kuro (stored in ~/.config/fish/functions/vi.fish)
set -Ux EDITOR kuro                # optional: tools that honour $EDITOR
# switch back:
functions -e vi; rm ~/.config/fish/functions/vi.fish; set -e EDITOR
```

Your existing vim/neovim setup is not modified.

## Configure

Everything is in one TOML file kept under version control: `config/default/config.toml`.

```fish
set -Ux KURO_CONFIG_DIR (pwd)/config/default     # run from the repo root
kuro notes.txt
```

or make a copy `config/me/config.toml` and point `KURO_CONFIG_DIR` at that directory. Every option, with type,
default and example, is in [docs/CUSTOMIZATION.md](docs/CUSTOMIZATION.md) (a test keeps it in sync with the code).
Invalid or unknown settings produce a warning (`:warnings`), never a crash.

For a VS Code feel: `[editor] modal = false`.

## Keybindings

Shortcuts marked ★ work in every mode.

| Key | Action |
|---|---|
| ★ Ctrl+S | Save (asks for a name if the buffer has none) |
| ★ Ctrl+Q | Quit; if there are unsaved changes asks *Save changes to FILE? [y]es / [n]o / [c]ancel* |
| ★ Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z | Undo / redo / redo (Ctrl+Shift+Z needs a terminal with the kitty keyboard protocol, e.g. foot, kitty, wezterm) |
| ★ Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste (no selection: copy/cut the whole line; pasting that puts it above the cursor line) |
| ★ Ctrl+A | Select all |
| ★ Alt+C / Alt+X / Alt+V | Same as Ctrl+C/X/V, for terminals or multiplexers that swallow Ctrl+C/X/V |
| ★ Ctrl+F / Ctrl+H | Find / find and replace |
| ★ F3 / Shift+F3 | Next / previous match |
| ★ Ctrl+P | Command line (same as `:`, works in Insert mode and in modeless mode) |
| ★ F5 / F6 / F7 | SQL: run selection / run whole buffer / cancel running query |
| ★ F4 | Focus the SQL result table (and back) |

**Ctrl+C and interrupts.** The editor puts the terminal in raw mode, so Ctrl+C is delivered as a key and never
sends SIGINT; it copies. If your terminal or tmux reserves Ctrl+C/X/V (or you press Ctrl+Shift+C by habit), use
Alt+C / Alt+X / Alt+V, or the mouse selection plus your terminal's own copy.

### Editing without modes (`editor.modal = false`, or Insert mode)

Arrows move; Shift+arrows select; Ctrl+arrows jump by word; Home toggles first non-blank / line start;
Ctrl+Home / Ctrl+End; PageUp / PageDown; Tab / Shift+Tab; Alt+Backspace deletes a word; Enter keeps the indent.
Esc leaves Insert mode in modal mode (and does nothing in modeless mode).

### Normal mode

| Keys | Action |
|---|---|
| `h j k l`, arrows | Move (counts work: `5j`) |
| `w b e`, `0 ^ $`, `gg G`, `NG`/`Ngg` | Word and line motions, go to line N |
| `i a I A o O` | Enter Insert mode (at / after cursor, line start / end, new line below / above) |
| `x X` | Delete char under / before cursor (`3x`) |
| `d{motion}` `y{motion}` `c{motion}` | Delete / yank / change over a motion; `dd yy cc` (`3dd`); `D C Y` |
| `p P` | Paste after / before (linewise registers paste whole lines) |
| `u`, Ctrl+R | Undo / redo |
| `v`, `V` | Visual / Visual-line mode: motions extend; `d x y c p o` act; Esc leaves |
| `:` | Command line |
| `/`, `n`, `N` | Open the find bar / next / previous match |
| Ctrl+D / Ctrl+U | Half page down / up |
| Esc | Cancel pending command / clear selection / close find bar |

This is a deliberate subset of vi (no macros, marks, `.` repeat, text objects or `f`/`t` motions).

### Command line

`:w [file]`, `:q`, `:q!`, `:wq`, `:x`, `:e file`, `:N` (go to line N), `:warnings`, and `:sql …` (below).

### Mouse (all modes, can be turned off with `editor.mouse = false`)

| Action | Result |
|---|---|
| Click | Place the cursor |
| Drag | Select |
| Double-click / triple-click | Select word / line |
| Click the line number | Select the whole line |
| Wheel (Shift+wheel) | Scroll vertically (horizontally) |
| Click find-bar buttons | Toggle Aa / ab / .* , previous ↑, next ↓, close ✕, replace ↵, replace all ⇉ |
| Over the result table | Wheel scrolls rows, Shift+wheel / horizontal wheel scrolls columns; click a header to select a column; double-click a header to expand it |

### Find bar

`Ctrl+F` opens it in the top-right corner: query field, `N of M` counter, and the toggles **Match case**
(`Alt+C`), **Whole word** (`Alt+W`), **Regex** (`Alt+R`). All matches are highlighted (underlined); the current
one is selected. `Enter` / `Shift+Enter` (or `F3` / `Shift+F3`, or ↓ / ↑) go to the next / previous match and
wrap around; `Esc` closes. `Ctrl+H` adds a replace row: `Tab` switches fields, `Enter` in the replace field replaces
the current match, `Alt+A` (or `Alt+Enter`) replaces all in one undo step. In regex mode `$1` works in the replacement.
`Shift+Enter` needs the kitty keyboard protocol; `F3`/`Shift+F3`/↑ work everywhere.

## SQL client

Supported databases: **PostgreSQL** and **MariaDB** only.

### Add a connection

1. Copy the example and edit it (it lives outside the repo; `connections.toml` is gitignored):

   ```fish
   mkdir -p ~/.config/kuro
   cp config/default/connections.example.toml ~/.config/kuro/connections.toml
   $EDITOR ~/.config/kuro/connections.toml     # or: kuro ~/.config/kuro/connections.toml
   ```

2. Credentials are **never stored in plaintext**. A profile may reference environment variables
   (`user = "env:PGUSER"`, `password = "env:PGPASSWORD"`) or ask for the password at connect time
   (`password = "prompt"`, input is masked). A literal password in the file is rejected.

   ```fish
   set -x PGUSER app; set -x PGPASSWORD (read -s -P 'password: ')   # for this shell only
   ```

3. In the editor: `:sql connect local_pg` (status bar shows `● postgres://user@host:port/db`).

Connections are **not encrypted** (no TLS support yet): use them for local/dev databases or through an SSH tunnel.

### Commands

| Command / key | Action |
|---|---|
| `:sql connect NAME` | Connect with profile NAME (replaces any current connection) |
| `:sql disconnect` | Cancel anything running and disconnect |
| `:sql run` or **F5** | Run the **selection** (select with `v`/`V`, Shift+arrows or the mouse) |
| `:sql buffer` or **F6** | Run the **whole buffer** |
| `:sql cancel` or **F7** | Cancel the running query (`pg_cancel_query` / `KILL QUERY`) |
| `:sql close`, `:sql show`, `:sql profiles` | Hide / show the result pane, list profiles |

### Results

Results appear in a pane under the editor (`sql.result_view = "split"`, the default; the editor stays
usable) or full-screen in front of it (`"tab"`; Esc / F4 flip back and forth).

* **Header** (column name and type) stays visible while scrolling.
* **Size limits**: at most `sql.table_max_width` cells wide (default 120) and `sql.table_max_rows` rows
  (default 25) visible at once, fewer if the terminal is smaller. Scroll for the rest.
* **Scrolling**: F4 focuses the table, then `j k`/arrows, PageUp/PageDown, `g`/`G` for rows;
  `h l` for columns; `H L` or Ctrl+←/→ by 8 cells; Home/End first/last column. Mouse: wheel, Shift+wheel.
* **Wide columns** are cut with `…` at `sql.column_max_width` (default 40). Press `e` (or Enter/Space) on the
  current column, or double-click its header, to expand it (up to the table width); again to collapse.
* `Ctrl+C` while the table is focused copies the result as TSV. `Esc`/`Tab` returns to the editor, `q` hides the pane.
* **Status line**: `200 rows · 5.5 ms` (time is measured around the database call; ms or s) or, for failures,
  `✖ syntax error · 2.0 ms · line 1, col 8` with the server's message above it.
* **Error categories**
  * *syntax error*: the statement could not be parsed
  * *runtime error*: it parsed but failed on data or types (division by zero, bad cast, constraint violation)
  * *execution error*: environment or object problems: missing table/column, permission denied, connection lost,
    timeout, cancelled; also the fallback for unknown error codes

  Line and column come from the database (PostgreSQL gives a position; MariaDB's `near '…' at line N` is
  located inside your script) and are mapped back to buffer coordinates even when only a selection was run.

## Development

```fish
make test               # everything: unit, UI, integration (needs Docker for the last group)
make test-unit          # find, buffer, table layout, error classification, config, docs check
make test-ui            # rendering/input on the cell buffer + the real binary in a pty
make test-integration   # real PostgreSQL and MariaDB in throwaway Docker containers
make lint               # fmt --check + clippy
./scripts/ci.sh         # what CI runs (use `sh scripts/ci.sh` in fish)
```

Integration tests start `postgres:16-alpine` and `mariadb:11` bound to `127.0.0.1` on random ports with a
random password, and remove them afterwards. They never connect to anything else. If Docker is not available
they print `SKIPPED integration test: docker is not available…` and pass; set `KURO_REQUIRE_DOCKER=1` (CI does)
to make that a failure. To refresh the recorded error fixtures from live servers:
`env KURO_RECORD_FIXTURES=/tmp/rec.txt make test-integration`, then review `/tmp/rec.txt` against
`tests/fixtures/db_errors.txt`.

### What the tests cover, and what they do not

| Layer | How | Covers |
|---|---|---|
| Unit | `cargo test --lib`, `classify_fixtures`, `docs_check` | find (case/word/regex/wrap/replace), buffer editing/selection/undo/clipboard round-trip, table layout (limits, truncation, scroll, header, empty, one row, wide/unicode), error classification on recorded server messages for both databases, config loading (defaults, unknown keys warn), docs ⇄ code option parity (with self-tests that the check really fails on drift) |
| UI (cell buffer) | `ui_render`, `ui_sql` | what is painted (never a background, ANSI colours only), find bar position/toggles/count/replace, mouse (click, drag, double/triple click, gutter, wheel, bar buttons), Ctrl+C/X/V/A, modal and modeless editing, table scroll/expand/limits, error pane, query flow with a fake driver (run, errors mapped to buffer lines, cancel), results tab |
| UI (real terminal) | `pty_ui`, `pty_sql` | the actual binary in a pseudo-terminal read through a terminal emulator (`vt100`): first paint, default background on every cell, find bar top-right, SGR mouse events, Ctrl+C/V, save/quit prompts, and a full PostgreSQL session (connect, run, scroll, expand, errors, cancel) |
| Integration | `integration_sql` | MariaDB 11 and PostgreSQL 16: connect, typed results, syntax / runtime (incl. type mismatch) / execution (missing table, permission denied) errors, wrong password, 200 000-row result with `row_limit`, long query + cancel, connection loss |

Not tested (be aware):

* the real Wayland/X11 clipboard helpers in CI (only the command selection logic and an in-memory clipboard). An opt-in
  check against the real clipboard: `env KURO_TEST_REAL_CLIPBOARD=1 cargo test --test clipboard_real`
  (it saves and restores a text clipboard);
* mouse behaviour and key encodings of any particular terminal emulator, tmux/screen, or ssh (the pty tests speak
  generic xterm SGR mouse and legacy key sequences);
* kitty-keyboard-only combinations (Ctrl+Shift+Z, Shift+Enter) against a real kitty-protocol terminal;
* database versions other than PostgreSQL 16 and MariaDB 11, TLS connections, and MySQL;
* very large files and result sets beyond the 200 000-row case.

## Layout

```
src/            app.rs (input), render.rs (drawing), buffer.rs, find.rs, findbar.rs, table.rs,
                syntax.rs, clipboard.rs, config.rs, sql/ (drivers, worker, profiles, classify)
config/default/ config.toml (all options) and connections.example.toml
docs/           CUSTOMIZATION.md, ENVIRONMENT.md, ARCHITECTURE.md
tests/          unit-ish fixtures, UI, pty, integration, docs check
```

See [CHANGELOG.md](CHANGELOG.md) for history.
