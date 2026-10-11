# kuro

A `vi`-style terminal editor that behaves like VS Code in usability, with a built-in SQL client for
**PostgreSQL** and **MariaDB**. It draws with your terminal's own background and ANSI palette: it never sets a
background colour and never emits truecolor or 256-colour sequences, so dark and light themes (and transparency) just work.

* modal editing (Normal / Insert / Visual) that can be switched off for a VS Code feel
* hard tabs by default (width 8), VS Code-style Tab / Shift+Tab / Backspace / Delete
* mouse everywhere: click, drag, double-click word, triple-click line, gutter click, wheel
* system clipboard: Ctrl+C / X / V / A (`wl-clipboard`, `xclip`, `xsel`, or OSC 52)
* VS Code-style find bar (top-right) with `3 of 12`, Match case / Whole word / Regex, and replace
* absolute line numbers with the current line highlighted (relative numbers are an option)
* syntax colouring for SQL, Go, Python, shell and JSON: operators, numbers, strings, comments, keywords and
  **bracket-depth colours** that cycle through ANSI slots you can remap; `:setlang` picks the language by hand
* word suggestions while typing, like VS Code's
* SQL client: **Ctrl+Enter** runs the selection or the statement under the cursor, per-driver snippets, a scrollable
  result table, timing, row count, warnings, classified errors (syntax / runtime / execution) with line and column, cancel

## Requirements

* Linux/macOS terminal that speaks xterm sequences (tested in `foot`; nothing is terminal-specific). For Ctrl+Enter a
  terminal with the **kitty keyboard protocol** is needed, otherwise use the fallback key
  ([which terminals](docs/KEYBINDINGS.md#which-terminals-can-send-ctrlenter))
* Rust 1.85+ (edition 2024) to build: `pacman -S rust` on Arch
* Optional: `wl-clipboard` (Wayland) or `xclip` / `xsel` (X11). Without them OSC 52 is used for copy.
* Optional: Docker, only to run the integration tests

## Install (fish)

```fish
git clone https://github.com/ArieqAqila/kuro-ide ~/src/kuro; and cd ~/src/kuro
make install                       # cargo install --path . + fish completions
fish_add_path ~/.cargo/bin         # once, if ~/.cargo/bin is not on your PATH yet
kuro --version                     # prints the git commit it was built from, e.g. kuro 0.1.0 (775d078)
```

After pulling or committing changes, run `make install` again and **restart any running kuro**: an installed
binary (and a process already running it) keeps the old behaviour until then. If `kuro --version` shows no commit or an
older one than `git log -1`, you are running a stale build.

Use it as `vi` **without touching the system `vi`** (see [docs/ENVIRONMENT.md](docs/ENVIRONMENT.md) for what `vi`
resolves to on this machine):

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

or copy it to `config/me/config.toml` and point `KURO_CONFIG_DIR` there. Every option, with type, default and example,
is in [docs/CUSTOMIZATION.md](docs/CUSTOMIZATION.md), including how to map a colour role to a different ANSI slot
(`[colors]`). A test keeps that file in sync with the code. Invalid or unknown settings produce a warning
(`:warnings`), never a crash.

For a VS Code feel: `[editor] modal = false`. For spaces instead of tabs: `indent_style = "spaces"`, `indent_size = 4`.

## Several terminals, one file

Open the same file in two or more kuro terminals and they stay in step like VS Code tabs: unsaved edits appear
everywhere, undo/redo and save are shared, and saving from any terminal writes everything. If a kuro process dies
nothing is lost (another one takes over; if all die, the next start restores the unsaved text). There is nothing to
start or install. Details and limits (files over `sync.max_file_mib`, CRLF files, conflicts with outside changes):
**[docs/SYNC.md](docs/SYNC.md)**.

## Keys

The complete, generated list is in **[docs/KEYBINDINGS.md](docs/KEYBINDINGS.md)**. The ones to know:

| Key | Action |
|---|---|
| Ctrl+S / Ctrl+Q | Save / quit (asks when there are unsaved changes) |
| Ctrl+Z, Ctrl+Y (Ctrl+Shift+Z) | Undo, redo |
| Ctrl+C / X / V / A | Copy, cut, paste, select all (Alt+C/X/V where the terminal reserves Ctrl) |
| Ctrl+F / Ctrl+H | Find / replace; Enter, Shift+Enter or F3, Shift+F3 step; Esc closes |
| Ctrl+Enter | SQL: run the selection, or the statement under the cursor (fallback: Alt+Enter) |
| Alt+L / `:runline` | SQL: run only the current line |
| Alt+S / `:snippets` | SQL snippets for the connected driver |
| `:setlang <mode>` | Language mode (a picker without an argument; `auto` to detect again) |
| `:connect NAME` | Connect with a profile |
| Ctrl+P or `:` | Command line |

**Ctrl+C is a copy, not an interrupt:** kuro uses raw mode, so Ctrl+C reaches the editor as a key. If your terminal or
multiplexer reserves Ctrl+C/X/V, use Alt+C/X/V.

## SQL client

Supported databases: **PostgreSQL** and **MariaDB** only.

### Add a connection

1. Copy the example (it lives outside the repo; `connections.toml` is gitignored):

   ```fish
   mkdir -p ~/.config/kuro
   cp config/default/connections.example.toml ~/.config/kuro/connections.toml
   $EDITOR ~/.config/kuro/connections.toml
   ```

2. Credentials are **never stored in plaintext**: a profile may reference environment variables
   (`user = "env:PGUSER"`, `password = "env:PGPASSWORD"`) or ask at connect time (`password = "prompt"`, masked).
   A literal password in the file is rejected. A password you type at the prompt is kept **in memory only** for the
   rest of the session, so reconnecting to the same profile does not ask again; it is wiped when kuro exits, dropped as
   soon as the server rejects it, and `:sql forget [profile]` clears it on demand. It is never written to disk or hashed.
   For example, for this shell only:

   ```fish
   set -x PGUSER app; set -x PGPASSWORD (read -s -P 'password: ')
   ```

3. In the editor: `:connect local_pg` (the status bar then shows `● postgres://user@host:port/db`).

Connections are **not encrypted** (no TLS yet): use them for local/dev databases or through an SSH tunnel.

### Running queries

* **Ctrl+Enter** runs the selection, or the statement under the cursor when nothing is selected (statements are split
  at `;` outside quotes and comments; PostgreSQL `$$` bodies and MariaDB backslash/`#` rules are honoured).
* **Terminals without the kitty keyboard protocol cannot send Ctrl+Enter** (it arrives as a plain Enter). kuro
  detects that at startup, tells you, and the **fallback key** (`sql.run_fallback_key`, default **Alt+Enter**) or
  `:run` does the same thing. [Which terminals support it](docs/KEYBINDINGS.md#which-terminals-can-send-ctrlenter).
* **Alt+L** / `:runline` runs just the current line, `:runbuffer` (or F6) the whole buffer, F7 / `:cancel` cancels.
  **F5 no longer runs anything.**
* **Not connected?** A SQL buffer stays in Normal mode and shows *Not connected to a database. Use :connect
  <profile>*; Insert mode and edits are refused until you connect (`sql.block_editing_when_disconnected`).
* **Snippets** (Alt+S, `:snippets`): lists differ per driver (`SHOW TABLES` / `DESCRIBE` / `LIMIT x, y` / `DATE_ADD` on
  MariaDB; `information_schema` / `FETCH FIRST` / `interval` / `ON CONFLICT` on PostgreSQL). Without a connection a
  generic list is shown and says so. PostgreSQL's psql commands (`\dt`, `\d`) are client commands, not SQL, so the
  snippets use `information_schema` instead. Every snippet is executed against both real servers in the tests.

### Results

Results appear in a pane under the editor (`sql.result_view = "split"`; the editor stays usable) or full-screen in front
of it (`"tab"`; Esc / F4 flip back and forth).

* The header (column name and type) stays pinned while scrolling vertically.
* At most `sql.table_max_width` cells wide (default 120) and `sql.table_max_rows` rows (default 25) are visible at once;
  scroll with the keyboard (F4, then arrows, `h`/`l`, PageUp/Down, `g`/`G`) or the mouse (wheel, Shift+wheel).
* Columns wider than `sql.column_max_width` (default 40) end in `…`; press `e` on the current column (or double-click
  its header) to expand it. SQL `NULL` is drawn dim and italic, distinct from the text `NULL`, with no background.
* **Status line**: `200 rows · 5.5 ms` (time is measured around the database call) or, for failures,
  `✖ syntax error · 2.0 ms · line 1, col 8` with the server's message above it. Warnings that do not fail the statement
  appear as `⚠ …` and are never reported as errors.
* **Error categories**: *syntax error* (could not parse), *runtime error* (parsed but failed on data or types: division by
  zero, bad cast, constraint violation), *execution error* (missing table or column, permission denied, connection
  lost, timeout, cancelled; also the fallback for unknown codes). Line and column come from the database and are mapped
  back to buffer coordinates, even when only a statement or selection was run.

## Development

```fish
make test               # everything (the last group needs Docker)
make report             # everything, with passed / failed / skipped per test layer
make test-unit          # logic, input parser, config and keybinding docs checks
make test-prop          # property / fuzz tests (make test-prop-long for the nightly-size run)
make test-snapshots     # golden screens
make test-ui            # cell-buffer UI tests and the real binary in a pty
make test-sync          # live sync: simulator, real sockets, several editors, several processes with SIGKILL
make test-integration   # real PostgreSQL and MariaDB in throwaway Docker containers
make lint               # fmt --check + clippy
make docs               # regenerate the keybinding tables
```

See **[docs/TESTING.md](docs/TESTING.md)** for the layers, the rules (deterministic, no fixed sleeps, isolated),
fixtures, what cannot be tested and why, and the log of changes to existing tests. Integration tests start
`postgres:16-alpine` and `mariadb:11` bound to `127.0.0.1` on random ports with a random password and remove them
afterwards; they never connect to anything else. Without Docker they print `SKIPPED …` and pass; set
`KURO_REQUIRE_DOCKER=1` (CI does) to make that a failure.

## Layout

```
src/            app.rs (input), render.rs (drawing), input.rs (terminal input parser), buffer.rs, find.rs, findbar.rs,
                indent.rs, suggest.rs, picker.rs, snippets.rs, syntax.rs, table.rs, keymap.rs, keyspec.rs, ansi.rs,
                clipboard.rs, config.rs, sql/ (drivers, worker, profiles, classify, stmt),
                sync/ (live sync: ot, replica, leader, node, cluster simulator, net, real files and journal)
config/default/ config.toml (all options) and connections.example.toml
docs/           CUSTOMIZATION.md, KEYBINDINGS.md, SYNC.md, TESTING.md, ENVIRONMENT.md, ARCHITECTURE.md
tests/          unit-level fixtures, property, snapshot, UI, pty, integration, docs checks
```

See [CHANGELOG.md](CHANGELOG.md) for history.
