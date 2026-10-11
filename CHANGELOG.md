# Changelog

All notable changes to this project are documented here. Format: [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Fixed
- Line numbers no longer restart from 1 above and below the cursor line while editing. Root cause:
  `editor.relative_numbers` defaulted to `true`, so every other line showed its distance from the cursor line.
  Absolute numbers are now the default (relative is opt-in). Regression tests:
  `regression_gutter_stays_absolute_*` in `tests/ui_render.rs`.

### Added
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

### Changed
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
