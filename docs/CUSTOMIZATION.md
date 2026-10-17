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

### `editor.word_suggestions`
- **Type:** boolean
- **Default:** `true`
- While typing in Insert mode (or in modeless mode) a popup suggests words already in the file, case-insensitively: words that start with what you typed first, then words that contain it, nearer ones first. Up/Down or Ctrl+N / Ctrl+P move, Tab or Enter accepts (replacing the typed prefix), Esc dismisses.
- **Example:** `word_suggestions = false`

### `editor.suggest_min_chars`
- **Type:** integer ≥ 1
- **Default:** `2`
- How many characters of a word must be typed before suggestions appear.
- **Example:** `suggest_min_chars = 3`

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
- Lines moved per mouse-wheel notch in the editor. A horizontal notch moves twice as many cells.
- **Example:** `scroll_lines = 5`

### `editor.scrollbars`
- **Type:** boolean
- **Default:** `true`
- Draw a vertical scrollbar (thin: one column of line glyphs; the thumb is a bold `┃`, the track a dim `│`) on the right edge and a horizontal one at the bottom of the editor. Each appears only when the text does not fit (more lines than rows, or a line wider than the window) and disappears again when it does. Click an arrow to step, click the track to page, drag the thumb to scroll; the thumb is drawn in reverse video and the track dim, never with a background colour.
- **Example:** `scrollbars = false`

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

### `sql.run_fallback_key`
- **Type:** string (a key such as `"alt+enter"`, `"f9"`, `"ctrl+r"`)
- **Default:** `"alt+enter"`
- Runs the selection, or the statement under the cursor, exactly like Ctrl+Enter. Needed in terminals that cannot send Ctrl+Enter (most can only send a plain Enter for it; see [KEYBINDINGS.md](KEYBINDINGS.md#which-terminals-can-send-ctrlenter)). kuro tells you when it detects such a terminal.
- **Example:** `run_fallback_key = "f9"`

### `sql.run_line_key`
- **Type:** string (a key such as `"alt+l"`)
- **Default:** `"alt+l"`
- Runs only the line the cursor is on (also available as `:runline`).
- **Example:** `run_line_key = "alt+shift+l"`

### `sql.block_editing_when_disconnected`
- **Type:** boolean
- **Default:** `true`
- In a SQL buffer with no database connection kuro stays in Normal mode, refuses edits and Insert mode, and shows "Not connected to a database. Use :connect <profile>". Inserting a snippet is still allowed. `false` lets you edit SQL files offline.
- **Example:** `block_editing_when_disconnected = false`

### `sql.auto_connect`
- **Type:** string (a profile name from the connections file)
- **Default:** `""`
- The first time a SQL buffer is shown and no connection exists, kuro runs `:connect` with this profile by itself. A profile with `password = "prompt"` asks for the password then, exactly as `:connect` would. It is tried once per kuro session, so after `:disconnect` or a failed connect it does not try again; use `:connect` yourself. Empty means never.
- **Example:** `auto_connect = "local_maria"`

## `[ui]`

### `ui.syntax_highlight`
- **Type:** boolean
- **Default:** `true`
- Colour code with the terminal's 16 ANSI palette colours (never RGB, never a background).
- **Example:** `syntax_highlight = false`

### `ui.kitty_keyboard`
- **Type:** string, `"auto"`, `"on"` or `"off"`
- **Default:** `"auto"`
- Use the kitty keyboard protocol so that Ctrl+Enter, Shift+Tab, Ctrl+Shift+Z, a bare Esc and Alt+letter can be told apart from other keys. `auto` asks the terminal at startup (a short, bounded query) and falls back to legacy keys if it does not answer; `on` assumes support without asking; `off` never uses it. Without the protocol Ctrl+Enter cannot be sent, so use `sql.run_fallback_key`.
- **Example:** `kitty_keyboard = "off"`

## `[sync]`

Live sync keeps every kuro terminal that has the same file open in step, unsaved edits included (like several
VS Code tabs of one file). See [SYNC.md](SYNC.md).

### `sync.enabled`
- **Type:** boolean
- **Default:** `true`
- Turn live sync on or off. When off, every terminal edits its own copy and saving still refuses to overwrite a file that changed on disk behind your back.
- **Example:** `enabled = false`

### `sync.max_file_mib`
- **Type:** integer (MiB)
- **Default:** `5`
- Files bigger than this open without live sync (the status bar says `local`), because every terminal keeps a full copy and an edit history of the text.
- **Example:** `max_file_mib = 20`

### `ui.adaptive_colors`
- **Type:** boolean
- **Default:** `true`
- Colours that an *extension* defines for its own roles (ksystem's local variables, calcs, file fields and indexes) start as the slots the extension declares. With this on, kuro asks the terminal for its real colours once at start-up (a short, bounded query, only when an extension is installed) and, for every role you have not mapped yourself, keeps the declared slot if it is readable on your theme and otherwise picks the palette slot that stands out best, or a text style (bold, underline, italic) if no slot does. It only ever chooses among the terminal's 16 palette slots and styles: kuro never invents a colour. Turn it off to always use the declared slots.
- **Example:** `adaptive_colors = false`

## `[history]`

The hard-undo history (see [HARD-UNDO.md](HARD-UNDO.md)): before kuro first writes a file it saves the file as it was, so `:hardundo` can put it back, even after every
kuro terminal was closed. The copies live in `~/.local/state/kuro/history/` (mode `0700`). **kuro never deletes anything there by itself**: use `kuro history delete` / `clear`.
A file with secrets in it is stored there too (as plain text), so keep `exclude` current or turn the history off.

### `history.enabled`
- **Type:** boolean
- **Default:** `true`
- Record the original of every file kuro writes. Off: nothing is recorded and `:hardundo` says it is off.
- **Example:** `enabled = false`

### `history.max_file_mib`
- **Type:** integer (MiB)
- **Default:** `5`
- Files bigger than this (before or after the write) are not recorded; kuro says so when you save one.
- **Example:** `max_file_mib = 20`

### `history.exclude`
- **Type:** list of strings
- **Default:** `["*.env", ".env*", "*.pem", "*.key", "id_*"]`
- File-name patterns (`*` and `?`, matched against the name only) that are never recorded. Setting it replaces the default list.
- **Example:** `exclude = ["*.env", "*.secret", "credentials*"]`

### `history.warn_mib`
- **Type:** integer (MiB)
- **Default:** `200`
- When the history is bigger than this kuro says so once at start-up (`history uses 340 MiB: kuro history list`). It only tells you; it deletes nothing. `0` turns the message off.
- **Example:** `warn_mib = 1000`

## `[git]`

`kuro git` (see [GIT.md](GIT.md)). In a repository with 100,000 commits, git needs many seconds to find the commits that touched one file. A *commit graph with
changed-path filters* makes that about a hundred times faster, and kuro keeps one for you **outside the repository**, in `$XDG_CACHE_HOME/kuro/git/<id>/`
(usually `~/.cache/kuro/git/`); the repository is never touched. It is built in the background the first time you open a file's history in a big repository
(about 20 seconds of one CPU core for 100,000 commits, about 6 MiB of disk) and rebuilt in the background when it is more than a day old. Delete the folder at any time.

### `git.graph_cache`
- **Type:** boolean
- **Default:** `true`
- Build and use the cache described above. Off: kuro asks git for the file history directly (slower in big repositories) and writes nothing.
- **Example:** `graph_cache = false`

### `git.graph_cache_min_commits`
- **Type:** integer
- **Default:** `5000`
- Repositories with fewer commits than this never get a cache.
- **Example:** `graph_cache_min_commits = 20000`

## `[ext.<name>]`

Per-extension settings, one table per installed extension (see [EXTENSIONS.md](EXTENSIONS.md)); not part of the option table above because the keys depend on the extension.

```toml
[ext.ksystem]
headers = ["/path/to/ksystem/*.h"]    # files the extension reads names from (here: the table headers)

[ext.ksystem.colors]               # role = palette slot 0-15, or -1 for the terminal's default text colour
local = 6
index = -1
```

* `[ext.<name>.colors]` remaps the roles the extension declares. A role you map here is never changed by `ui.adaptive_colors`. A slot outside -1..15 is a warning.
* Any other key is a list of paths the extension asked for. Only the file name part may contain `*`, `?` or `[a-z]`.

## `[colors]`

kuro never emits RGB, 256-colour or background colours. Every text colour is a **role** mapped to one of the
terminal's 16 ANSI palette slots, so the terminal theme decides what it looks like (a light and a dark theme
get different actual colours from the same config). Emphasis (selection, current match, focused column,
mismatched brackets) uses reverse video, bold, dim or underline, never a background colour.

To map a role to a different slot, set it to a number: `0-7` are black, red, green, yellow, blue, magenta,
cyan, white; `8-15` are the bright versions; `-1` means "the terminal's default foreground".

```toml
[colors]
keyword = 12              # bright blue keywords
string = 10               # bright green strings
bracket_depth = [9, 11, 10, 14, 12, 13]
```

Out-of-range values produce a warning and use the default. `tests/color_rule.rs` fails the build if any
rendered screen contains a truecolor, 256-colour or background-colour escape sequence.

### `colors.keyword`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `5`
- Language keywords (`func`, `SELECT`, `if`, …).
- **Example:** `keyword = 12`

### `colors.builtin`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `4`
- Built-in functions and types, and shell variables.
- **Example:** `builtin = 14`

### `colors.literal`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `3`
- `true`, `false`, `nil`, `null`, `None`.
- **Example:** `literal = 11`

### `colors.string`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `2`
- Single-, double- and raw-quoted strings.
- **Example:** `string = 10`

### `colors.number`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `6`
- Numbers in decimal, hex (`0x1F`), binary, octal and exponent (`1e-5`) forms.
- **Example:** `number = 14`

### `colors.operator`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `1`
- Operators such as `=`, `==`, `!=`, `&&`, `||`, `:=`, `->`.
- **Example:** `operator = 9`

### `colors.comment`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `-1`
- Comments. `-1` draws them dim and italic in the default foreground, which stays readable in every theme.
- **Example:** `comment = 8`

### `colors.key`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `4`
- JSON object keys.
- **Example:** `key = 12`

### `colors.bracket_error`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `1`
- Unmatched or mismatched brackets; they are also shown in reverse video.
- **Example:** `bracket_error = 9`

### `colors.bracket_depth`
- **Type:** list of integers `0`-`15` (ANSI slots), not empty
- **Default:** `[3, 5, 6, 2, 4]`
- Bracket pairs `() [] {}` are coloured by nesting depth, cycling through this list. Brackets inside strings and comments are ignored.
- **Example:** `bracket_depth = [9, 11, 10, 14, 12, 13]`

### `colors.diff_add`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `2`
- `kuro git`: added lines in a diff.
- **Example:** `diff_add = 10`

### `colors.diff_del`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `1`
- `kuro git`: removed lines in a diff.
- **Example:** `diff_del = 9`

### `colors.diff_hunk`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `6`
- `kuro git`: the `@@ -1,3 +1,4 @@` hunk headers.
- **Example:** `diff_hunk = 14`

### `colors.diff_meta`
- **Type:** integer, `-1` or `0`-`15` (ANSI slot)
- **Default:** `-1`
- `kuro git`: the file headers of a diff (`diff --git`, `index`, `---`, `+++`, `rename from`). `-1` draws them bold in the default text colour.
- **Example:** `diff_meta = 12`

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
| `KURO_EXT_REGISTRY` | Git URL of an extension registry (a repository with one folder per extension), used by `kuro ext install NAME`. Not set by default: install from the extension's own git URL (see [EXTENSIONS.md](EXTENSIONS.md)). |

Test-only variables (`KURO_REQUIRE_DOCKER`, `KURO_RECORD_FIXTURES`) are described in the README.

## Languages (`:setlang`)

`:setlang <name>` (or the picker, `:setlang` alone) sets the language mode and recolours at once; `:setlang auto` goes back
to detection by file name, extension or `#!` line. The name is case-insensitive and the aliases below work too.
Every mode colours keywords, builtins, literals, strings, comments, numbers and operators, and colours `() [] {}` by
nesting depth with unmatched or mismatched ones in reverse video (markup text itself has no brackets).

| Mode | `:setlang` aliases | Detected by | Notes |
|---|---|---|---|
| Plain Text | `text`, `plain`, `txt`, `none` | anything else | no colours |
| SQL | `postgres`, `mysql`, `mariadb` | `.sql` | |
| Go | `golang` | `.go`, `go.mod` | |
| Python | `py`, `python3` | `.py`, `.pyi`, `.pyw`, `#!python` | |
| Shell | `sh`, `bash`, `zsh`, `fish`, `ksh` | `.sh`, `.bash`, `.zsh`, `.fish`, `.ksh`, `.bashrc`, `.zshrc`, `.profile`, `.bash_profile`, `config.fish`, `#!sh` / `#!bash` / … | |
| JSON | | `.json`, `.jsonc`, `.geojson`, `.eslintrc`, `.prettierrc` | object keys are keys |
| Lua | `luajit` | `.lua`, `.rockspec`, `.luacheckrc`, `#!lua` | `--[[ ]]` block comments; long strings `[[ ]]` are not recognised |
| Rust | `rs` | `.rs` | `'a` lifetimes are builtins, `'x'` characters are strings; nested block comments and raw strings `r#"…"#` are not recognised |
| C | | `.c`, `.h` | `#include` / `#define` lines: the directive is a keyword |
| C++ | `cpp`, `c++`, `cxx`, `cc` | `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx`, `.ipp` | `.h` is C |
| Java | | `.java` | `@Annotations` are builtins |
| JavaScript | `js`, `node`, `nodejs` | `.js`, `.mjs`, `.cjs`, `.jsx`, `#!node` / `#!deno` / `#!bun` | regex literals and template strings, see below |
| TypeScript | `ts` | `.ts`, `.mts`, `.cts`, `.tsx` | JavaScript plus types |
| HTML | `htm`, `xhtml` | `.html`, `.htm`, `.xhtml`, `.shtml` | embeds CSS and JavaScript |
| CSS | | `.css` | |
| PHP | `phtml` | `.php`, `.phtml`, `.php3`, `.php4`, `.php5`, `.phps`, `#!php` | HTML with PHP regions |
| Ruby | `rb` | `.rb`, `.rake`, `.gemspec`, `.ru`, `Rakefile`, `Gemfile`, `Guardfile`, `Vagrantfile`, `#!ruby` | `@ivar`, `$global` are builtins; `=begin`/`=end` comments are not recognised |
| YAML | `yml` | `.yml`, `.yaml`, `.clang-format`, `.gitlab-ci.yml` | `key:` at the start of a line is a key |
| TOML | | `.toml`, `Cargo.lock`, `poetry.lock`, `Pipfile` | `[section]` lines are keywords, `key =` is a key |
| XML | `svg` | `.xml`, `.xsd`, `.xsl`, `.xslt`, `.svg`, `.plist`, `.rss`, `.atom`, `.wsdl`, `.csproj`, `.pom` | |

### How HTML, XML, PHP and CSS are coloured

These are small state machines (`src/markup.rs`), not parsers: they colour what they recognise and leave the rest
plain, so a half-typed file is never a problem.

| What | Role (a palette slot in `[colors]`) |
|---|---|
| tag names, `<!DOCTYPE>`, `<?php` / `<?=` / `?>`, CSS element selectors and `@rules`, `!important` | keyword |
| attribute names, CSS properties and `--custom-properties`, media features | key |
| attribute values (quoted or not), CDATA, strings | string |
| `<` `>` `</` `/>` `=`, CSS combinators (`>` `+` `~`) | operator |
| `<!-- -->` and `/* */` comments | comment |
| `&amp;` / `&#169;` entities, CSS `.class` and `#id` selectors | literal |
| CSS numbers with units (`1.5em`, `50%`) and `#rrggbb` colours | number |
| CSS `:hover` / `::before`, CSS functions (`url(`, `rgba(`, `calc(`), XML `<?xml ?>` | builtin |

**Embedded regions.** A region is handed to the language it contains and tokenized on its own:

* `<style>…</style>` → CSS. `<script>…</script>` → JavaScript (also with `type="module"`, `text/javascript`,
  `application/javascript`); `type="application/json"` / `ld+json` / `importmap` → JSON; any other `type`
  (`text/template`, …) is left plain.
* In a `.php` file, `<?php … ?>`, `<?= … ?>` and a bare `<?` … `?>` → PHP; the markers are keywords. PHP regions also work
  inside tags and attribute lists.
* A region that is never closed runs to the end of the file. A comment or string never leaks out of its region, and an
  unclosed `<!--` comments the rest of the file, as in a browser.
* XML has no embedded languages: `<style>` and `<script>` are ordinary elements.

**JavaScript details.** Template strings (`` `…` ``) may span lines and are one string (`${…}` is not parsed
separately). A `/` starts a regular-expression literal, coloured as a string with its flags, when it appears where a value
is expected (after `=`, `(`, `,`, `:`, `[`, `!`, `&`, `{`, `;`, an operator, or `return`/`typeof`/`case`/…); after an
identifier, number, `)` or `]` it is division. A regex after `)` (`if (x) /re/.test(y)`) is therefore read as division, a known limit.

## Adding a syntax-highlighting language

Highlighting is table-driven (`src/syntax.rs`); there is no per-language code.

1. Add a `LangSpec` next to the existing ones (`src/syntax.rs`, or `src/langs.rs` for the newer ones). Fields: `name` (shown in the status bar), `extensions`,
   `filenames`, `shebangs` (interpreter basenames), `keywords`, `builtins`, `literals`, `line_comments`,
   `block_comment`, `string_quotes`, and flags `triple_quotes` (Python-style), `raw_quote` (Go-style backtick),
   `case_insensitive`, `shell_vars`, `key_colon` (JSON keys), `operators` (the operator tokens; the longest match wins),
   `brackets` (colour `() [] {}` by depth) and `bracket_errors` (flag unmatched ones). Newer flags: `regex_literals` (JavaScript),
   `sigils` (`$name`, `@name`), `bare_key_sep` (`:` for YAML, `=` for TOML), `section_headers` (TOML `[section]`), `lifetimes`
   (Rust `'a`), `preprocessor` (C `#include`) and `markup` (HTML, XML, PHP, CSS: a state machine in `src/markup.rs`).
   Start with `..BASE` and set only what differs.
2. Append `&YOUR_LANG` to the `LANGS` list.
3. Add tests (`tests/syntax_langs.rs` has the helpers), a snapshot (`lang_snapshot!` in `tests/snapshots.rs`), a row in the table above
   (`tests/docs_check.rs` fails when a mode or one of its extensions is missing), and update `every_language_is_registered_and_named_uniquely`.
4. `make test-unit`.

Colours are chosen by role in `kind_style` in `src/render.rs` (configured through `[colors]`); use palette slots or modifiers only.
