# Design proposal: extension system and the `ksystem` language (Major Update 2)

Status: **proposed, waiting for approval.** Nothing is implemented yet.

## Decisions I need from you

1. **Data only, or code too?** Recommended for this first version: an extension contains **declarative language definitions
   only** (a manifest, TOML): no scripts, no plugins, no WebAssembly, no network, nothing kuro executes. That makes the
   sandbox boundary trivial to state and to test ("an extension is data and cannot run"), and it covers the stated
   long-term plan (language modes first). Executable extensions would be a separate, later format version with their own
   approval.
2. **What does "nothing outside kuro may load or run an extension" have to mean in code?** Because an extension is data, it
   cannot *run* anywhere. For *loading* I propose two enforcements and one honest limit:
   * **Only kuro's installer can create a loadable extension.** `kuro ext install` validates the source, then writes it into
     kuro's own extension directory together with a *seal*: an HMAC-SHA256 over the manifest and every file, keyed with a
     random 32-byte secret that is created on first install in `~/.local/state/kuro/ext.key` (mode `0600`) and never leaves
     the machine. kuro loads an extension only if the seal verifies; a copied, edited or hand-made directory does not load,
     and kuro has **no** flag or environment variable to load an extension from a path.
   * **The loader is private to the kuro binary** (not part of the library's public API), checks that the manifest's `format`
     is one it knows, and refuses extensions whose declared kuro version range does not include this kuro.
   * **Limit:** the files are readable text and the secret sits in your home directory, so another program running as *you*
     can read the definitions, and could forge a seal if it read the key. This stops accidental or casual reuse and
     tampering, not a determined same-user attacker. If you want stronger (for example encrypted payloads), say so; it adds
     complexity without real security against the same user.
3. **Source format of `kuro ext install <source>`:** recommended **a local directory** containing `kuro-ext.toml` (what you
   would clone or unpack). Archives (`.tar.gz`) and URLs add dependencies and a trust question, so I would leave them for
   later; say if you need them now.
4. **How does the ksystem extension get detected?** I have not read the reference code yet (the sandbox that runs my commands
   refused to read `/home/ksystem/prtgitks` file contents: *"PII data handling"*). Its files use ordinary extensions
   (`.html`, `.htm`, `.xml`, `.txt`, `.var`, …), so an extension rule has to be able to say **"this `.html` is ksystem"**.
   Proposal: a language may list `extensions`, `filenames`, `shebangs` **and** `first_line_regex` / `contains` markers (a
   small number of leading lines are checked), plus optional `path_globs`; the order is: exact file names, then extension
   markers (content sniff), then extension file types, then core file types. **I need you to either allow me to read that
   directory (a permission rule for `Read`/`Bash` on it) or give me anonymised samples**; the fixtures in `tests/fixtures/`
   will be small anonymised excerpts either way, and nothing under `/home/ksystem` is ever modified or copied into the repo.

## Manifest

`kuro-ext.toml` at the root of the extension directory:

```toml
format = 1                     # manifest schema version; kuro rejects any it does not know
name = "ksystem"               # [a-z0-9-], 1-40 chars; also the directory name
version = "0.1.0"              # the extension's own version (semver)
kuro = ">=0.1, <0.2"           # kuro versions it works with (checked against kuro --version)
description = "ksystem language"
license = "MIT"

[[language]]
name = "ksystem"               # what :setlang accepts (case-insensitive) and the status bar shows
aliases = ["ks"]
extensions = ["var"]
filenames = []
first_line_regex = '^<!--\s*ksystem'       # content marker (optional)
keywords = ["…"]               # same fields as the built-in languages: builtins, literals, line_comments,
line_comments = ["//"]         # block_comment = ["/*", "*/"], string_quotes, operators, brackets, …

[[language.rule]]              # extra rules for things a word list cannot express
pattern = '\$\{[^}]*\}'        # Rust `regex` syntax: linear time, no backtracking blow-ups
role = "builtin"               # a core role (keyword builtin literal string number comment key operator) or one of this extension's own roles

[[language.declares]]          # names declared in one place and coloured wherever they are used in the file (ksystem needs this)
section = '^PGM_FIELDS'        # the declaration is in lines after a line matching this, up to the next section header
name = '^(l[A-Za-z0-9_]*)'     # regex on the line; group 1 is the declared name
role = "local"                 # an own role (declared below): every use of that exact name gets this role

[[language.names_from]]        # names that live in the *user's own files* (ksystem: the table headers); see below
setting = "headers"            # the user lists the files in config.toml: [ext.ksystem] headers = ["/path/*.h"]
parser = "c-struct-members"    # one of a fixed set of built-in parsers (never code from the extension):
                               #   c-struct-members  `TYPE name;` / `TYPE name[N];` inside typedef struct
                               #   c-negative-defines  `#define name -1601` (ksystem index and table ids)
role = "field"                 # every occurrence of a name found there
```

**What an extension may contain:** only `[[language]]` tables (and, later, other declarative tables under new `format`
numbers). **Colours are roles, never colours:** the only way to colour text is `role = …` (the eight core roles, plus any **named roles the extension declares itself**, see below), which `[colors]` /
`[ext.<name>.colors]` map to ANSI
palette slots 0-15 or the default foreground, so extension text follows the terminal theme exactly like the core editor.
There is no field that can express RGB, a 256-colour index or a background, and unknown keys are rejected with a message
naming the key.

Limits (so a bad extension cannot hurt kuro): manifest ≤ 256 KiB, ≤ 16 languages, ≤ 2 000 words per list, ≤ 100 rules, each
regex ≤ 1 KiB and compiled with a size limit; names restricted to a safe alphabet; no symlinks or `..` in the source.

## Install, list, remove

```fish
kuro ext install ./extensions/ksystem   # validate, copy, seal, record
kuro ext list                           # name, version, languages, kuro range, where it came from
kuro ext remove ksystem                 # delete the files and the record
```

* Installed under `$XDG_DATA_HOME/kuro/extensions/<name>/` (default `~/.local/share/kuro/extensions/`), with a record in
  `registry.toml` next to it: name, version, source path, install time, seal, kuro version at install. The registry is
  replaced atomically under a lock; a crash leaves either the old or the new state, and `kuro ext list` reports and offers to
  remove a half-installed entry.
* Installing the same name again asks (`--force` replaces); removing an unknown name is an error; nothing else on disk is touched.
* At start kuro loads installed extensions, **skips (and says why in `:warnings`) any that fail the seal, the format or
  the version check** instead of failing to start. Uninstalled means: its files simply are not detected any more.
* The extension is not installed by default ("optional"). The ksystem extension's source lives in the repository under
  `extensions/ksystem/` (our own rules and README, **not** a copy of the reference code).

## How it plugs into the existing code

`LANGS` stays the core list; a new `syntax::registry()` returns core languages plus loaded extension languages, and
`detect`, `by_name`, `:setlang` and the picker use it. Extension language definitions are turned into the same `LangSpec`
(their strings are leaked once per process, bounded by the limits above), so highlighting, bracket colours, snapshots and the
colour-rule tests apply to them unchanged. `[[language.rule]]` adds an optional regex pass to the generic tokenizer.

## Roles an extension declares, and how the user maps them

An extension may declare **named roles** with a default palette slot; they are the extension's own colour vocabulary:

```toml
[language.roles]               # name = default ANSI slot (0-15) or -1 for the terminal's default text colour
local = 14
calc  = 11
field = 10
index = 13
```

The user remaps them per extension, in their own config, with the same slot numbers `[colors]` uses (so they follow the terminal theme
like everything else; no RGB, no 256-colour, no background):

```toml
# ~/.config/kuro/config.toml
[ext.ksystem.colors]
local = 6
index = -1                     # -1: the terminal's default text colour
```

Unknown role names in that table are a `:warnings` entry. For ksystem the defaults are four *distinct* bright slots (14, 11, 10, 13)
so that locals, calcs, file fields and indexes tell apart out of the box, and since a slot is the terminal theme's own colour, light
and dark themes each pick a legible one.

### Adaptive default colours (`ui.adaptive_colors`, default on)

The declared slot is only a starting point. At start-up kuro asks the terminal for its real colours (xterm's `OSC 4 ; n ; ?` for slots
1-15 and `OSC 10` / `OSC 11` for the default text and background, one bounded query like the keyboard check) and, **for every extension
role the user has not mapped**, picks the palette slot that is most distinct from the background, from the default text colour and
from the other roles already placed (greedy, by perceptual distance). A dark theme and a light theme therefore get different
slots, and a terminal whose palette is poor (few distinct slots, or a slot that nearly equals the background) is handled too:
when no slot is distinct enough the role falls back to a **style** (bold, italic, underline, or a mix) on a good slot instead of a
look-alike colour.

What this deliberately does **not** do: *invent* colours. kuro never writes RGB or 256-colour sequences (the hard rule stays), so it can
only choose among the terminal's own 16 slots and the text styles; "a new colour that matches the terminal" is achieved by choosing
the matching slot or style, never by emitting a colour. If the terminal does not answer the query (tmux without passthrough, some
multiplexers, `ui.adaptive_colors = false`) the declared slots are used. A role the user maps in `[ext.<name>.colors]` is never
changed. The choice is a pure function of (palette, background, default text, roles), so it is unit-tested without a terminal, and a
pty test feeds it an emulated palette.

## Names from the user's own files (`names_from`)

ksystem file fields are defined in C headers (`/home/ksystem/*.h`, ≈140 files, ≈5 000 members, e.g. `TEXT almnama[51];`). Shipping that list
would copy their schema into our repository, so the extension instead lets **the user** point kuro at the files:

```toml
# ~/.config/kuro/config.toml
[ext.ksystem]
headers = ["/home/ksystem/*.h"]
```

The extension may only *declare* that it wants names from a setting and which **built-in parser** reads them (`c-struct-members`:
`TYPE name;` / `TYPE name[N];` inside `typedef struct`, all other lines ignored). `c-negative-defines` finds the **index** (and table id) names in the same headers (`#define almprmidx -1601`). Hard limits: the paths come only from the user's
config (an extension cannot name a path), regular files only, at most 4 000 files / 64 MiB / 100 000 names, read-only, nothing is ever
executed, and a missing or unreadable path is a `:warnings` entry, never a failure. Names are cached in `~/.local/state/kuro/` keyed by
file size and modification time and read lazily the first time a ksystem file is shown.

## The ksystem language (after approval)

1. Study `/home/ksystem/prtgitks` (read-only): which of `.html`, `.htm`, `.rptdesign`, `.xml`, `.txt`, `.var` and the extension-less
   directories (`buat_git`, `coba123`, `debug2`, `html`, `scan`) use which syntax; find the keywords, directives, operators,
   comments and strings.
2. Record the findings in `extensions/ksystem/README.md` (file type → syntax), then write the rules.
3. Fixtures: small anonymised excerpts in `tests/fixtures/ksystem/`; no real data, names or credentials.

## Tests planned

install / list / remove (real temp dirs; registry atomicity and a crash between copy and record); reinstall and `--force`;
a copied or edited extension directory fails the seal and is skipped with a warning; no way to load from a path; an invalid
manifest (bad TOML, unknown key, unknown `format`, wrong kuro range, role that is not a role, RGB-looking colour field,
oversized, `..` in a name, symlink) is rejected with a clear message; extension colours only ever use the 8 roles (the colour
rule test runs over extension languages); detection precedence; the ksystem tokenizer on the fixtures; docs checks for the
manifest fields and the `kuro ext` commands.
