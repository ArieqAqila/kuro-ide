# Changelog

All notable changes to this project are documented here. Format: [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

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
