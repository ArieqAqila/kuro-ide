# Customization

All customization lives in one TOML file. Keep it in this repository so every change is reviewable:

```fish
# use the committed defaults directly (edit config/default/config.toml, review with git diff)
set -Ux KURO_CONFIG_DIR (pwd)/config/default
# ...or keep your own copy next to it, e.g. config/me/config.toml, and point at that directory
set -Ux KURO_CONFIG_DIR (pwd)/config/me
```

Resolution order for the config directory: `--config FILE` (file, not directory) → `$KURO_CONFIG_DIR` →
`$XDG_CONFIG_HOME/kuro` → `~/.config/kuro`. A missing file means "all defaults".

**Bad config never crashes the editor.** Unknown keys, wrong types, bad enum values and invalid TOML
produce a warning (shown as a hint in the bottom line; run `:warnings`) and the default is used.

A test (`tests/docs_check.rs`) fails if an option exists in code but not in this file, or the reverse,
or if a documented default differs from the real one. Headings below are the keys; keep them in the form
``### `section.key` `` with a `Default` line.

## `[editor]`

### `editor.modal`
- **Type:** boolean
- **Default:** `true`
- vi-style Normal / Insert / Visual modes. `false` gives a VS Code feel: always inserting, Shift+arrows select, Esc only clears the selection. Mouse and Ctrl shortcuts work in both.
- **Example:** `modal = false`

### `editor.tab_width`
- **Type:** integer ≥ 1
- **Default:** `8`
- Display width of a tab character (tab stops are every `tab_width` columns). This only changes how existing tabs are drawn.
- **Example:** `tab_width = 4`

### `editor.indent_style`
- **Type:** string, `"tabs"` or `"spaces"`
- **Default:** `"tabs"`
- What the Tab key and indenting a selection insert: hard tab characters, or spaces.
- **Example:** `indent_style = "spaces"`

### `editor.indent_size`
- **Type:** integer ≥ 1 (columns)
- **Default:** `8`
- With `indent_style = "spaces"`: spaces per indent step (Tab inserts up to the next multiple of this; Shift+Tab and Backspace in leading indentation step back by it). With tabs, one step is always one tab character.
- **Example:** `indent_size = 4`

### `editor.smart_whitespace_delete`
- **Type:** boolean
- **Default:** `true`
- Backspace after text removes the whole run of spaces/tabs before the cursor, and Delete removes the whole run after it, in one keypress (Backspace in leading indentation always removes one indent step instead). `false` deletes one character at a time.
- **Example:** `smart_whitespace_delete = false`

### `editor.line_numbers`
- **Type:** boolean
- **Default:** `true`
- Show the line-number gutter. The current line's number is bold; clicking the gutter selects the line.
- **Example:** `line_numbers = false`

### `editor.relative_numbers`
- **Type:** boolean
- **Default:** `false`
- Numbers are absolute (1, 2, 3, …) by default. `true` switches to hybrid relative numbers: the current line shows its absolute number, the others their distance from it. Ignored when `line_numbers` is off.
- **Example:** `relative_numbers = true`

### `editor.mouse`
- **Type:** boolean
- **Default:** `true`
- Capture the mouse (click, drag, double/triple click, wheel). Turn off to let the terminal handle selection itself.
- **Example:** `mouse = false`

### `editor.double_click_ms`
- **Type:** integer (milliseconds)
- **Default:** `400`
- Maximum time between clicks (on the same cell) that counts as a double / triple click.
- **Example:** `double_click_ms = 250`

### `editor.scroll_lines`
- **Type:** integer ≥ 1
- **Default:** `3`
- Lines moved per mouse-wheel notch in the editor.
- **Example:** `scroll_lines = 5`

## `[clipboard]`

### `clipboard.backend`
- **Type:** string, one of `"auto"`, `"wl-clipboard"`, `"xclip"`, `"xsel"`, `"osc52"`
- **Default:** `"auto"`
- `auto` picks `wl-copy`/`wl-paste` on Wayland, `xclip` then `xsel` on X11, and falls back to the OSC 52 terminal escape (copy only; paste then uses the last text copied inside the editor). Use `osc52` over SSH or in terminals where the helper programs are unavailable.
- **Example:** `backend = "osc52"`

## `[find]`

Initial state of the find-bar toggles (they can still be flipped with Alt+C / Alt+W / Alt+R or the mouse).

### `find.case_sensitive`
- **Type:** boolean
- **Default:** `false`
- **Example:** `case_sensitive = true`

### `find.whole_word`
- **Type:** boolean
- **Default:** `false`
- **Example:** `whole_word = true`

### `find.regex`
- **Type:** boolean
- **Default:** `false`
- Treat the query as a regular expression (Rust `regex` syntax; `$1` works in the replacement).
- **Example:** `regex = true`

## `[sql]`

### `sql.result_view`
- **Type:** string, `"split"` or `"tab"`
- **Default:** `"split"`
- Where results appear. `split` is a pane under the editor, which stays editable. `tab` shows results full-screen in front of the editor as soon as a query finishes; Esc or F4 flips back to the editor, F4 again returns to the results.
- **Example:** `result_view = "tab"`

### `sql.table_max_width`
- **Type:** integer ≥ 1 (terminal cells)
- **Default:** `120`
- The result table never shows more than this many cells across, even on a wider terminal; scroll horizontally for the rest.
- **Example:** `table_max_width = 100`

### `sql.table_max_rows`
- **Type:** integer ≥ 1
- **Default:** `25`
- The table never shows more than this many data rows at once (it also shrinks to the available height); scroll vertically for the rest.
- **Example:** `table_max_rows = 15`

### `sql.column_max_width`
- **Type:** integer ≥ 1 (terminal cells)
- **Default:** `40`
- Columns wider than this are cut with `…`. Press `e` (or double-click the header) on the current column to expand it, up to `table_max_width`.
- **Example:** `column_max_width = 30`

### `sql.row_limit`
- **Type:** integer ≥ 1
- **Default:** `10000`
- Rows kept in memory per result. The status line reports the real total and says when the result was cut.
- **Example:** `row_limit = 50000`

### `sql.connections_file`
- **Type:** string (path)
- **Default:** `""`
- Where connection profiles are read from. Empty means `<config dir>/connections.toml`. Never commit this file (it is in `.gitignore`).
- **Example:** `connections_file = "/home/me/.config/kuro/connections.toml"`

## `[ui]`

### `ui.syntax_highlight`
- **Type:** boolean
- **Default:** `true`
- Colour code with the terminal's 16 ANSI palette colours (never RGB, never a background).
- **Example:** `syntax_highlight = false`

### `ui.kitty_keyboard`
- **Type:** boolean
- **Default:** `true`
- Ask the terminal for the kitty keyboard protocol so combinations like Ctrl+Shift+Z can be told apart from Ctrl+Z. Terminals without it ignore the request; switch off if your terminal misbehaves.
- **Example:** `kitty_keyboard = false`

## Connection profiles (`connections.toml`)

Not part of `config.toml`, and never committed. Copy `config/default/connections.example.toml` to
`<config dir>/connections.toml`. Each profile is a `[profiles.NAME]` table; connect with `:sql connect NAME`.
Any value except `driver` may be `"env:VARNAME"` to read an environment variable.

**Passwords are never stored.** `password` may only be `"env:VARNAME"` or `"prompt"` (asked for, masked, when
you connect). A literal password is rejected with an error.

### `profile.driver`
- **Type:** string, `"postgres"` or `"mariadb"` (required)
- **Example:** `driver = "postgres"`

### `profile.host`
- **Type:** string
- **Default:** `"127.0.0.1"`
- **Example:** `host = "env:PGHOST"`

### `profile.port`
- **Type:** integer or `"env:VARNAME"`
- **Default:** `5432` for postgres, `3306` for mariadb
- **Example:** `port = 5433`

### `profile.user`
- **Type:** string (required)
- **Example:** `user = "env:PGUSER"`

### `profile.password`
- **Type:** `"env:VARNAME"` or `"prompt"`; omit for no password
- **Example:** `password = "env:PGPASSWORD"`

### `profile.database`
- **Type:** string
- **Default:** the server's default
- **Example:** `database = "app"`

## Environment variables

| Variable | Purpose |
|---|---|
| `KURO_CONFIG_DIR` | Directory holding `config.toml` and `connections.toml` (overrides `~/.config/kuro`). |

Test-only variables (`KURO_REQUIRE_DOCKER`, `KURO_RECORD_FIXTURES`) are described in the README.

## Adding a syntax-highlighting language

Highlighting is table-driven (`src/syntax.rs`); there is no per-language code.

1. Add a `LangSpec` next to the existing ones. Fields: `name` (shown in the status bar), `extensions`,
   `filenames`, `shebangs` (interpreter basenames), `keywords`, `builtins`, `literals`, `line_comments`,
   `block_comment`, `string_quotes`, and flags `triple_quotes` (Python-style), `raw_quote` (Go-style backtick),
   `case_insensitive`, `shell_vars`, `key_colon` (JSON keys). Start with `..BASE` and set only what differs.
2. Append `&YOUR_LANG` to the `LANGS` list.
3. Add a test in the `tests` module of `src/syntax.rs` (see the existing `kind_of` helper) and update
   `every_language_is_registered_and_named_uniquely`.
4. `make test-unit`.

Colours are chosen in `kind_style` in `src/render.rs`; use ANSI palette colours or modifiers only.
