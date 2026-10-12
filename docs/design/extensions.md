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
role = "builtin"               # one of: keyword builtin literal string number comment key operator variable variable2

[[language.declares]]          # names declared in one place and coloured wherever they are used in the file (ksystem needs this)
section = '^PGM_FIELDS'        # the declaration is in lines after a line matching this, up to the next section header
name = '^(l[A-Za-z0-9_]*)'     # regex on the line; group 1 is the declared name
role = "variable"              # every later or earlier use of that exact name gets this role
```

**What an extension may contain:** only `[[language]]` tables (and, later, other declarative tables under new `format`
numbers). **Colours are roles, never colours:** the only way to colour text is `role = …` (ten roles: the eight above plus `variable` and
`variable2`, two new `[colors]` entries added for this), which `[colors]` maps to ANSI
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
