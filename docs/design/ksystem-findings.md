# ksystem reference code: what I found

Read-only study of `/home/ksystem/prtgitks` (nothing there was modified, copied or committed). It becomes the README of the ksystem
extension once the extension design is approved. Business names, people, hosts and field names from the reference are **not**
reproduced here; the fixtures will be small anonymised excerpts.

## Which file types use which syntax

| Files (count) | What they are | Syntax |
|---|---|---|
| `sct<N>` with **no extension** in the top folder (≈2 200), `template/tmp_*` (≈3 000), `scan/s*` (≈50) | **ksystem screen / program definitions: the ksystem language** | ksystem (below) |
| `*.var` (4) | tables of variables: `name TYPE length CODE label`, whitespace or tab separated, one per line | uses the ksystem type names (`STRING`, `INT2`, …) only; a small, separate "var table" rule set |
| `*.html` (≈320), `*.htm` (≈300) | documentation exported from LibreOffice (`<!DOCTYPE HTML 4.0>`, `<font>`, `<p align=…>`) plus a few hand-written fragments (`<h1>`, `<img>`) | plain **HTML**: the built-in HTML mode is right; **not** ksystem |
| `*.rptdesign` (31) | BIRT report designs | **XML** (built-in XML mode) |
| `bhs*.xml` (2) | message tables (`<messages><message id="…">…`) | **XML** |
| `*.man` (9), `*.txt` (8) | prose: rules, guides, notes | plain text |
| `buat_git`, `debug2`, `ambilsct.sh`, `coba123` (empty), `html` (a broken symlink) | shell notes, a log, a one-line script, nothing | shell / plain text; not ksystem |
| `*.png`, `birtlib.tgz`, `sample` | binary | none |

So **ksystem lives in files whose names are `sct<digits>`, `tmp_*` or inside `scan/`**, i.e. a *file-name* rule (and a content
marker: they begin with `SCREEN …`, `IMAGE(…)`, `MAP(…)`, `FIELD(…)`, `CALCS(…)` or a `/*****` banner), **not** an extension rule.
Two of the extensions the extension has to deal with, `.html` and `.htm`, are NOT ksystem, so the extension must not claim them.

## The ksystem language

A line-oriented, case-sensitive definition language. A file is a sequence of **sections**; most lines are whitespace-separated columns.

* **Header:** `SCREEN <number> QUERY_TYPE <name>`, optional `SCR_FILE`, `SCR_LOGFLD`, `SCR_JOIN_FILE<n>`, … (all `SCR_*`), ends
  with `END_SCREEN`.
* **Sections** open with `NAME(argument)` at the start of a line: `MAP`, `DBCALCS`, `CALCS`, `SPCALCS`, `IMAGE`, `FIELD` / `FIELDS`,
  `EDITS` / `EDIT`, `DEFAULTS` / `DEFAULT`, `HOOKS`, `SUBFILE`, `POSTING`, `PERMIT`, `RTREE`, `USES_SCRIPT`, `HELP`, `SFL_TEMPLATE`,
  `SFL_HEADER`, `SFL_FOOTER`, plus argument-less blocks `LOCAL` and `PGM_FIELDS`. In the sample (≈7 000 files) `MAP(` is by far
  the most common, then `IMAGE(`, `CALCS(`, `DBCALCS(`, `SUBFILE(`.
* **Keyword families** (the words that open or fill lines): `SFL_*` (sub-file options), `DBC_*` (`DBC_COMMAND`, `DBC_TARGET`, `DBC_MAP`,
  `DBC_COND`, `DBC_SELECT`, `DBC_SUM_MAP`, …), `PST_*` / `PMT_*` (posting / permit conditions and messages), `SCR_*`, `RT_*` (report
  output: `RT_HTML`, `RT_XLSX`, `RT_PDF`, `RT_COND`, `RT_MAP`), hooks `BEFORE_INPUT`, `AFTER_INPUT`, `HKMSG`, `HKCURSOR`.
* **Actions:** `REPLACE`, `REPLACE_IF`, `NO_REPLACE`, `DO_CALC`, `DO_CALC_IF`, `DO_LIB`, `DO_LIB_IF`, `DO_MAP`, `DO_SCAN`, `DO_SCAN_IF`,
  `DO_SKIP_IF`, `DO_SPCALCS`, `DO_FLY`, `SHOW`, `INIT`, `INIT_ADD`, `INIT_UPDATE`, `DFALT_LIST`, `DFALT_SCAN`, `IF` / `ELSE`.
* **Field attributes** (columns of `FIELD`): `INPUTRI` (followed by an order number), `PROTECT`, `QPROTECT`, `NOCHANGE`, `ALLCAPS`, `SCROLL`,
  `NUMERIC`, `TEXTAREA`, `NOLIMIT`, `UNSEEN`, `TOMBOL`, `CHECKBOX`, `DIGITS`, `MAND_PLUS`, `MANDATORY`, `MANDATORY_IF`, `VALIDATE`, …
* **Types** (in `PGM_FIELDS`, `.var`): `STRING`, `NUMERIC`, `INT2`, `INT4`, `DATE`, `TIME`.
* **Comments:** only `/* … */`, spanning lines (≈11 000 uses; also the `/****…***/` banner). *(Confirmed by you: there is no `//` and no
  `#` comment. The ≈160 lines that start with `#` are therefore not comments; they are drawn as plain text.)*
* **Variables (confirmed by you):** two kinds. **Local** variables are declared under `PGM_FIELDS` (first column, name starts with `l`).
  **Calcs** are declared under `CALCS()` (first column, name starts with `c`); a calc is a variable without a type (it holds a condition, a
  number or a string). They get **different colours**. Both are *used* in many other places (`MAP` columns, `IF cond …`, `DO_LIB_IF cond …`).
* **Strings:** double quotes, `"…"`, which may span lines (embedded SQL such as `INIT "select … from …"` runs over several lines);
  single quotes occur mostly inside those SQL strings.
* **Operators** (conditions in `CALCS`, e.g. `x==1 && y!="HL" || (a>=b)`): `==`, `!=`, `<>`, `<=`, `>=`, `<`, `>`, `&&`, `||`, `=`, and
  `+ - * /`; `&name` passes a variable by address in `DO_LIB(…)` argument lists; `{name}` is a template substitution; `$` and `%` appear
  inside strings.
* **Calls:** `DO_LIB_IF cond fn(a,b,&c,…)` and builtin functions in calculations (`THS(...)`, `DATE_FORMAT`, `CONCAT`, `COALESCE`,
  `DATEDIFF`, `DATE_ADD`, `ROUND`, …); SQL keywords (`select from where join group by`, …) appear in strings and `DBC_*` values.
* **`IMAGE(name)` blocks** are ASCII-art screens until the next section: box lines start with `/---`, `|`, `\---`, and fields are `i.Label`
  (input), `t.Label` (text) and runs of `_` (the input slots). They are drawing, not code: colour only the labels, the underscores and
  the border characters lightly (or leave them plain).
* **Identifiers** are lowercase with a type prefix by convention (`l…` local, `gv_…` global value, `c…` condition, `d…` db-calc, `m…` map)
  but that is a convention, not syntax.

## File fields (the C headers)

`/home/ksystem/*.h` (139 files, 6.9 MB). Most are table definitions: `typedef struct alm { TEXT almdata[11]; LONG almprm; double almlat; … }`
(types `TEXT`, `LONG`, `double`, `COUNT`), one header per table, **every member prefixed with the file's name** (`alm.h` → `almdata`,
`almnama`, …), plus a block of index names (`almprmidx`, …). `error.h` and `posting.h` are logs/text, not definitions. Fields appear in ksystem
files as the first column of `FIELD(…)`/`MAP(…)` rows and in conditions, so they get a **third colour**; the index names
(`#define almprmidx -1601`, listed at the top of each header in an `INDEX_INFORMATION` block) get a **fourth**.

## What this means for the extension

1. Detection rule: **file names** `sct[0-9]*`, `tmp_*`, files inside a folder named `scan`, **or** a first-lines marker
   (`^(SCREEN\s|IMAGE\(|MAP\(|FIELD\(|CALCS\(|/\*{3,})`); `.var` as a second, smaller mode (or a rule inside the same mode, to be decided).
   `.html` / `.htm` / `.xml` / `.rptdesign` stay with the built-in modes.
2. Rules: section headers as keywords (`^[A-Z_]+\(`), the action and family words as keywords/builtins, types as builtins,
   `/* */` as the only comment, local and calc variables in two colours (see the design: declared names, two new colour roles), `"…"` strings spanning lines, the operators above, numbers, `&name` as a builtin, `{name}`
   as a literal, `IMAGE` bodies handled by a dedicated rule (label / underscore runs).
3. This needs two things the current tokenizer does not have, both inside the proposed extension format: a **section-aware** rule
   (a regex rule anchored at line start) and a **multi-line string** (`"` that continues). The `[[language.rule]]` regex rules in the
   design cover the first; the second is the `multiline_strings = true` language flag (the built-in tokenizer already has it for backticks).

## Limits of this study

I read file *structure* and samples, not all ≈7 000 files line by line; the vocabulary above is from frequency counts over all of
them (words seen ≥ 20 times), so rarer directives will be missing and shown as plain text. I have no ksystem grammar or manual, so
"keyword" vs "builtin" is by role as inferred from use. These are the things I could **not** verify against a real ksystem.
