# ksystem (kuro extension)

Colours the ksystem screen definition language. Install: `kuro ext install extensions/ksystem` (from the kuro repository), then open a
`sct<number>` or `tmp_<name>` file, a `.var` file, or `:setlang ksystem`.

Written from a read-only study of a ksystem source tree (about 7 000 files); **none of that code is in this repository**, and the test
fixtures are small invented files that only imitate its structure.

## Which files use which syntax

| Files | What they are | Mode |
|---|---|---|
| `sct<N>` (no extension), `tmp_*`, files under `scan/` | screen / program definitions | **ksystem** |
| `*.var` | variable tables: `name TYPE length CODE label` | ksystem (the type names) |
| `*.html`, `*.htm` | documentation exported from a word processor | HTML |
| `*.rptdesign`, `*.xml` | BIRT report designs, message tables | XML |
| `*.man`, `*.txt`, shell notes | prose | plain text |

Detection: file names `sct<digits>` and `tmp_<name>`, the extension `.var`, and (last) a first line that starts with `SCREEN `, `IMAGE(`,
`MAP(`, `FIELD(`, `CALCS(`, `DBCALCS(` or a `/****` banner, for files with no usable name such as those under `scan/`. A file with a known
type (`.html`, `.c`, `.sh`, …) is never taken over by the marker.

## The language

* **Sections** open with `NAME(argument)` at the start of a line (`MAP(`, `IMAGE(`, `CALCS(`, `DBCALCS(`, `FIELD(`, `HOOKS(`, `SUBFILE(`,
  `POSTING(`, `PERMIT(`, `RTREE(`, …) or are bare words (`LOCAL`, `PGM_FIELDS`); the header word is a keyword.
* **Keyword families:** `SFL_*`, `DBC_*`, `PST_*`, `PMT_*`, `SCR_*`, `RT_*`, `SCAN_*`; hooks (`BEFORE_INPUT`, `AFTER_INPUT`, …) and actions
  (`REPLACE`, `REPLACE_IF`, `DO_CALC`, `DO_LIB_IF`, `DO_SCAN`, `INIT`, …).
* **Field attributes and types** are builtins: `INPUTRI`, `PROTECT`, `NOCHANGE`, `ALLCAPS`, `SCROLL`, … and `STRING`, `NUMERIC`, `INT2`, `INT4`, `DATE`, `TIME`.
* **Comments are only `/* … */`** and may span lines. There is no `//` and no `#` comment.
* **Strings** are `"…"` and may span lines (embedded SQL); nothing inside a string or comment is coloured as a variable.
* **Operators:** `== != <> <= >= && || < > =` and arithmetic; `&name` (by address) and `{name}` (template) have their own colours.
* **`IMAGE(…)` blocks** are ASCII-art screens: the `i.Label` / `t.Label` words and the `______` input slots are coloured inside them.

## Four kinds of names, four colours

| Role | What | Where it is declared | Default slot |
|---|---|---|---|
| `local` | local variables, names starting with `l` | the first column under `PGM_FIELDS` | 14 (bright cyan) |
| `calc` | calcs: variables without a type (they hold a condition, a number or a string), names starting with `c` | the first column under `CALCS()` | 11 (bright yellow) |
| `field` | file fields | the table headers (`TEXT xxname[21];`) | 10 (bright green) |
| `index` | indexes and table ids | the same headers (`#define xxidx -1601`) | 13 (bright magenta) |

Locals and calcs are coloured only when the file really declares them (a name that merely starts with `l` or `c` is not), and then
**everywhere** in that file. Fields and indexes come from the headers you list in `~/.config/kuro/config.toml`:

```toml
[ext.ksystem]
headers = ["/home/ksystem/*.h"]

[ext.ksystem.colors]          # optional: your own slots (0-15, or -1 for the terminal's default text colour)
local = 6
```

The defaults are palette slots, so your terminal theme decides how they look, and with `ui.adaptive_colors` (on by default) kuro swaps a
slot that is hard to read on your theme for one that stands out; a role you map yourself is never changed.

## What was not verified

No ksystem grammar or manual was available, so which words are "keywords" and which "builtins" is inferred from how they are used; words
seen fewer than about 20 times in the sample are not listed and show as plain text. The language was checked against invented fixtures, not
against real ksystem files.
