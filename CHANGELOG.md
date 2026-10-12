# Changelog

All notable changes to this project are documented here. Format: [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Fixed
- Keyboard selection in **normal mode** now reaches the end of the line: starting a backward selection (Shift+Left,
  Shift+Ctrl+Left, Shift+Home, Shift+Up, Shift+PageUp) while the cursor rests on the last character takes that character too,
  so `I Love Claude!` can be selected whole. Regression tests: `tests/ui_select_keys.rs`.
- Horizontal scrolling with the mouse: the terminal's horizontal wheel (buttons 6/7 in SGR, X10 and urxvt reporting)
  and Shift + vertical wheel scroll the view sideways, clamped between the start and the end of the longest line
  (before, the view could be scrolled past the text). X10 and urxvt mouse reports are now parsed. Tests:
  `tests/ui_hscroll.rs` and the input parser tests.
- Ctrl + right-click on spaces or tabs moves the cursor to the end of that whitespace run (mixed tabs and spaces are
  one run); the rule is in `docs/KEYBINDINGS.md`. Tests: `tests/ui_ctrl_rightclick.rs`.
- Mouse selection (drag) now extends to the true end of the line, including trailing spaces and tabs, in every mode,
  and always matches the reverse-video highlight. A drag selects up to the nearest character *boundary*: dragging
  past the end of a line selects the whole line, and ending on the right half of a tab or a wide character includes it
  (before, the tab or the last character was only included once the mouse was past it). Tests: `tests/ui_mouse_select.rs`.
- Shift + arrows, Home/End and PageUp/PageDown now select in **normal mode** (they used to run the plain vi motion and
  select nothing); Shift+Ctrl+arrows select by word. A selection can be extended or shrunk in both directions.
  Regression tests: `tests/ui_select_keys.rs`.
- Moving up/down with the arrow keys (and `j`/`k`, PageUp/PageDown, Ctrl+D/U) keeps the **display** column, not the character
  index: a tab counts as many cells as it expands to (`editor.tab_width`) and wide characters count as their width, so
  the cursor no longer lands before `DO` on the next line of tab-separated fields. A column inside a tab or wide
  character goes to the nearer edge. Regression tests: `tests/ui_cursor.rs`.
- Line numbers no longer restart from 1 above and below the cursor line while editing. Root cause:
  `editor.relative_numbers` defaulted to `true`, so every other line showed its distance from the cursor line.
  Absolute numbers are now the default (relative is opt-in). Regression tests:
  `regression_gutter_stays_absolute_*` in `tests/ui_render.rs`.

### Added
- **More languages for `:setlang` and detection by extension**: Rust, C, C++, Java, JavaScript, TypeScript, HTML, CSS, PHP,
  Ruby, YAML, TOML and XML (and Lua), each with keyword, builtin, string, comment, number and operator rules and bracket
  depth colouring. HTML and PHP colour their embedded regions (`<style>` as CSS, `<script>` as JavaScript or JSON, `<?php … ?>`
  as PHP); the rules are in `docs/CUSTOMIZATION.md` ("Languages"), checked against the code by `tests/docs_check.rs`.
  JavaScript has template strings and regex literals; CSS has selectors, properties, values and at-rules.
- **Scrollbars**: a vertical bar on the right edge and a horizontal bar at the bottom of the editor, each shown only when
  the text does not fit. Arrows step by one line or column, clicking the track pages, dragging the thumb scrolls; the
  thumb is the visible fraction of the content (never a background colour: reverse video thumb, dim track). Turn them
  off with `editor.scrollbars = false`. Tests: `src/scrollbar.rs`, `tests/ui_scrollbars.rs`, snapshots at 80x24 and 120x40.
- Lua syntax highlighting and `:setlang lua` (`.lua`, `.rockspec`, `#!…lua` files; `--` and `--[[ ]]` comments).
- **Live sync** between terminals that have the same file open (like VS Code tabs): unsaved edits appear everywhere,
  undo/redo and save are shared, the cursor and scroll position stay on the same text, and nothing is lost when a kuro
  process is killed (leader hand-over, recovery window, journal). New `[sync]` options `enabled` and `max_file_mib`
  (default 5), `:w!` to overwrite a file that changed on disk, atomic saves with a safety check for every file. A SQL
  file is shared the same way. See `docs/SYNC.md`.
- SQL passwords are remembered in memory only, per profile/host/port/user/database, and reused on reconnect; cleared
  on exit, `:sql forget` and a wrong password; never written to disk, logged or hashed.
- Hard tabs by default: `editor.indent_style` (`tabs`/`spaces`), `editor.indent_size`, and `editor.smart_whitespace_delete`.
- VS Code-style Tab / Shift+Tab / Backspace / Delete: a selection over several lines (or one whole line) indents/outdents
  every touched line as one undo step; Shift+Tab with no selection outdents the current line; Backspace in leading
  indentation removes one indent step, not one space; Backspace/Delete on a whitespace run remove the whole run.

- Syntax: operators (`==`, `&&`, `||`, `:=`, `->`, `::`, …), numbers in hex/binary/octal/exponent forms, and
  bracket-depth colouring that cycles through configurable ANSI slots; unmatched and mismatched brackets are flagged in
  reverse video; brackets inside strings and comments are ignored.
- `[colors]` section mapping colour roles to ANSI slots 0-15 (documented in `docs/CUSTOMIZATION.md`).
- `src/ansi.rs`: an output filter that rewrites crossterm's `38;5;N` palette colours to the classic `30-37`/`90-97`
  codes, so no 256-colour sequence is ever emitted. `tests/color_rule.rs` and a pty test enforce the colour rule.

- `:setlang <mode>` (and a picker when run without an argument) sets the language mode and recolours immediately;
  `:setlang auto` returns to detection by extension. The status bar marks a manual mode.
- SQL snippets per driver (`:snippets` or Alt+S): MariaDB (`SHOW TABLES`, `DESCRIBE`, `LIMIT x, y`, `DATE_ADD`, …),
  PostgreSQL (`information_schema`, `FETCH FIRST`, `interval`, `ON CONFLICT`, …); with no connection a generic list is
  shown and says so. The integration tests execute every snippet on real servers.

- Word suggestions while typing (`editor.word_suggestions`, `editor.suggest_min_chars`): case-insensitive, prefix matches
  before substring matches, nearest to the cursor first; Up/Down or Ctrl+N / Ctrl+P move, Tab or Enter accepts
  (replacing the typed prefix), Esc dismisses; the popup only opens by typing and closes when the cursor leaves the word.

- **Input parser** (`src/input.rs`): kuro now reads raw terminal bytes itself (legacy keys, kitty keyboard protocol
  `CSI u`, SGR mouse, bracketed paste) so sequences split across reads are reassembled and an incomplete sequence never
  produces a key. Startup asks the terminal whether it speaks the kitty protocol (`ui.kitty_keyboard = "auto"`).
- **Ctrl+Enter** runs the selection, or the statement under the cursor when nothing is selected (`src/sql/stmt.rs` splits
  statements: quotes, comments, MariaDB backslashes and `#`, PostgreSQL `$$` bodies). `sql.run_fallback_key`
  (default `alt+enter`) does the same in terminals that cannot send Ctrl+Enter, and kuro says so when it detects one.
  `:runline` / `sql.run_line_key` (default `alt+l`) runs only the current line. `:connect`, `:disconnect`, `:run`,
  `:runbuffer`, `:cancel` are available as top-level commands.
- **Disconnected state**: a SQL buffer with no connection stays in Normal mode, refuses edits and Insert mode and shows
  "Not connected to a database. Use :connect <profile>" (`sql.block_editing_when_disconnected`).
- Property and fuzz tests (`tests/prop.rs`); the long run found and fixed a table-alignment bug for emoji with
  variation selectors (widths are now measured per grapheme).

- SQL `NULL` cells are drawn dim and italic (no background), distinct from the text `NULL`.
- Warnings that do not fail a statement (MariaDB `SHOW WARNINGS`, PostgreSQL notices) show in the status line and
  via `:sql warnings`; they are never reported as errors. Driver `before_statement`/`after_statement` hooks keep the
  extra round trips out of the measured query time.

- Test infrastructure: snapshot tests (22 golden screens), property/fuzz tests with a nightly long run, an injected click
  clock, a keybinding registry (`src/keymap.rs`) whose generated tables in `docs/KEYBINDINGS.md` and per-binding behaviour
  probes are checked against the code, `make report` (passed/failed/skipped per layer), `docs/TESTING.md`.

- `kuro --version` includes the git commit it was built from (`kuro 0.1.0 (775d078)`; `-dirty` with uncommitted changes), so a stale installed binary is easy to spot.

- **Password reuse (memory only):** a password typed for a `password = "prompt"` profile is remembered for the session
  and reused when reconnecting to the same profile (same server, user and database); other profiles ask again; a rejected
  password is never kept; it is wiped on exit and by `:sql forget [profile]`. Never on disk, never logged, never hashed;
  the process asks the kernel not to dump its memory once a password is stored.

### Removed
- **F5** no longer runs a query (use Ctrl+Enter, its fallback key, or `:run`). F6 (run buffer) and F7 (cancel) remain.

### Changed
- `ui.kitty_keyboard` is now a string (`auto` | `on` | `off`, default `auto`) instead of a boolean.
- `editor.tab_width` now defaults to `8` (was `4`); `editor.expand_tab` is replaced by `editor.indent_style`.
- `editor.relative_numbers` now defaults to `false`.
- The gutter width follows the digit count of the line count (grows at 10, 100, 1000 lines) and stays stable while scrolling.

## [0.1.0]

### Added
- Terminal editor (Rust, `ratatui` + `crossterm`, rope buffer) that renders with the terminal's own
  background and ANSI palette only.
- Modal editing (Normal / Insert / Visual / Visual-line, counts, operators `d y c`, registers); switch off
  with `editor.modal = false` for a VS Code feel.
- Mouse: click, drag, double-click word, triple-click line, gutter click selects a line, wheel scrolling.
- Clipboard: Ctrl+C / X / V / A via `wl-clipboard`, `xclip`, `xsel` or OSC 52; Alt+C/X/V alternatives.
- VS Code-style find bar (top-right) with `N of M`, Match case / Whole word / Regex, wrap-around navigation,
  highlight of all matches, and replace / replace all (single undo step).
- Line-number gutter with current line highlighted and relative numbers (default on).
- Undo/redo, save, quit-with-unsaved-changes prompt, status bar (file, modified marker, position, file type).
- Syntax highlighting for SQL, Go, Python, shell and JSON via a table-driven tokenizer.
- SQL client for PostgreSQL and MariaDB: connection profiles with `env:` / `prompt` credentials,
  run selection / whole buffer, cancel, scrollable result table (bounded width and rows, ellipsis + expand,
  sticky typed header), timing and row count, errors classified as syntax / runtime / execution with line and column.
- TOML configuration with warnings for unknown keys; `docs/CUSTOMIZATION.md` kept in sync by a test.
- Test suite: unit, UI (cell buffer and pty), and Docker-based integration tests; Makefile targets and CI workflow.
