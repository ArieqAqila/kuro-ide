# Changelog

All notable changes to this project are documented here. Format: [Keep a Changelog](https://keepachangelog.com/).

Short version. Every change in full: [docs/CHANGELOG-DETAILED.md](docs/CHANGELOG-DETAILED.md).

## [Unreleased]

### Changed
- `kuro git` diffs mark **what changed inside a line** (reverse video on the words that differ, on the removed and the added line).
- `kuro git` is much faster on big repositories: a file's history no longer uses `git log --follow`, builds a commit graph in `~/.cache/kuro/git` (never in the repository; `[git] graph_cache`), and on a 108,000-commit repository the first commit appears after 0.14 s (0.9 s the first time) instead of 28 s.
- `kuro git`: a file's history opens at once and fills in while git is still reading, like gitk (on a repository of 100,000 commits the first commit appears after about a second instead of after 30 seconds); it is read in one pass instead of three; leaving the history stops git. `docs/GIT.md` explains how to speed git up with a commit graph.
- `kuro git` outside a repository now prints `this directory is not in a git repository` (it was an Indonesian message, `direktori tidak ada git`); exit status 128 as before.

### Changed
- The scrollbars have no arrow buttons any more (the whole bar is track: a click pages, the thumb drags). They are thin: one column / one row of line glyphs (the thumb a heavier line `┃` / `━`, the track a light `│` / `─`), no more reverse-video blocks, also in `kuro git`.

### Added
- Alt + wheel scrolls sideways (in the text and in the result table), for terminals such as foot that keep Shift+wheel for their own text selection.

### Fixed
- `:hardundo` now discards **unsaved edits** too, in every terminal on the file and also those recovered from an earlier session; before, it said "nothing to undo" for a file kuro had not saved.

## [0.2.0]

Major update (0.1.0 → 0.2.0). The bundled ksystem extension now declares `kuro = ">=0.2, <0.3"`.

### Added
- **Hard undo**: `:hardundo` / `:hardundo all` restore files to how they were before kuro first wrote them, even after every terminal closed; `kuro history list|show|delete|clear`; nothing is automatic.
- **`kuro git`** and `kuro git -fl FILE`: read-only history browser (tree, file history with renames, commits, diffs). Outside a repository: `this directory is not in a git repository`, exit 128.
- **Extension system**: `kuro ext install|remove|list`, data-only language packs, sealed so only kuro loads them.
- **ksystem extension**: own colours for locals, calcs, file fields and indexes; `ui.adaptive_colors` follows your terminal's palette.
- **Scrollbars** (vertical and horizontal), clickable and draggable.
- **Languages**: Rust, C, C++, Java, JavaScript, TypeScript, HTML, CSS, PHP, Ruby, YAML, TOML, XML, Lua; embedded CSS/JS/PHP in HTML and PHP.
- **Live sync** between terminals editing the same file; atomic saves; `:w!`.
- SQL: Ctrl+Enter / `:runline`, statement splitting, snippets per driver, warnings, passwords kept in memory only.
- Editing: word suggestions, VS Code-style Tab/Backspace, hard tabs by default, operators and bracket-depth colouring, `[colors]`, `:setlang`.
- Input parser (kitty keyboard protocol, SGR/X10/urxvt mouse), property/fuzz and snapshot tests, `make report`.

### Fixed
- Keyboard selection in normal mode reaches the end of the line; Shift+arrows, Home/End, PageUp/PageDown select in normal mode.
- Mouse drag selects to the true end of the line; horizontal mouse scrolling (wheel buttons 6/7 and Shift+wheel); Ctrl+right-click on whitespace.
- Up/Down keep the display column with tabs and wide characters.
- Line numbers no longer restart from 1 (absolute by default).

### Changed
- `editor.tab_width` default 8 (was 4); `editor.expand_tab` replaced by `editor.indent_style`; `editor.relative_numbers` default `false`; `ui.kitty_keyboard` is `auto|on|off`.

### Removed
- F5 no longer runs a query (use Ctrl+Enter or `:run`).

## [0.1.0]

First version: modal editor with VS Code-like usability, mouse, clipboard, find/replace, syntax highlighting (SQL, Go, Python, shell, JSON), a PostgreSQL and MariaDB client with a result table, TOML configuration, and unit, UI and Docker integration tests.
