# Extensions

kuro has an **optional** extension system. For now an extension is **data only**: a manifest with language definitions (for `:setlang`, file
detection and colouring). kuro never runs anything from an extension, and extensions are not installed by default.

```fish
kuro ext install https://github.com/ArieqAqila/ksystem-kuro-ext   # from a git repository (URL, or URL@branch / tag / commit)
kuro ext install ksystem                          # by name, from a registry you set with KURO_EXT_REGISTRY (optional)
kuro ext install ./my-extension                   # from a folder
kuro ext list                                     # name, version, languages, status, where it came from
kuro ext remove ksystem                           # delete it
```

## Where extensions live

**Not in the kuro repository**, so kuro stays small however many there are. The source of an extension is a folder with a `kuro-ext.toml` (and a README) in one of three places:

| `SOURCE` | Meaning |
|---|---|
| `URL` or `URL@REF` | A git repository (https, http, ssh, git or file URL, or `git@host:path`) with `kuro-ext.toml` at its root. `REF` is a branch, tag or commit |
| a folder | A folder of your own. If a folder exists with that name it wins over a registry name |
| `NAME` (lower-case letters, digits, `-`) | The folder `NAME/` of a **registry** you chose: a git repository with one folder per extension, set with `set -x KURO_EXT_REGISTRY https://…`. There is no default registry |

kuro needs the `git` command for the first two. It downloads **one commit, without history**, into a temporary folder, unpacks only the tracked files (no `.git`, no hooks),
and deletes the folder afterwards. **Before it installs a download it shows what it is and asks**:

```text
install `ksystem` 0.1.0 (languages: ksystem) from https://github.com/ArieqAqila/ksystem-kuro-ext? [y/N]
```

`--yes` skips the question (and is required without a terminal, e.g. in a script); `--force` replaces an installed extension. A download then goes through exactly the checks
of a local install: valid manifest, a compatible kuro version, no links, no hidden files, at most 64 files, 4 MiB, nested at most 4 deep. Nothing in it is ever executed (an
extension is data), the URL cannot start a program (git's `ext::` transport and option-looking URLs are refused), and `list` says where each extension came from. What
`~/.local/share/kuro/extensions/` holds is only the installed, sealed copies; deleting a source changes nothing about them.

## The manifest, `kuro-ext.toml`

```toml
format = 1                     # manifest schema version; kuro rejects any it does not know
name = "ksystem"               # a-z 0-9 -, 1-40 characters; also the folder name
version = "0.1.0"
kuro = ">=0.2, <0.3"           # kuro versions it works with, checked against `kuro --version`
description = "…"
license = "MIT"

[[language]]                   # up to 16
name = "ksystem"               # what :setlang accepts (case-insensitive)
aliases = ["ks"]
extensions = ["var"]           # without the dot
filenames = ["Makefile.x"]
filename_globs = ["sct[0-9]*"] # `*`, `?`, `[a-z]` against the file name
filename_regex = ['^tmp_[A-Za-z0-9_]+$']
shebangs = ["ksh"]
first_line_regex = '^SCREEN\s' # last resort: how the file starts
keywords = ["IF"]
builtins = ["STRING"]
literals = ["NULL"]
operators = ["==", "&&"]
line_comments = ["//"]
block_comment = ["/*", "*/"]
string_quotes = ["\""]
multiline_strings = true       # a string may continue on the next line
brackets = true                # colour () [] {} by depth
bracket_errors = false         # flag unmatched ones
case_insensitive = false

[language.roles]               # own colour roles: name = default palette slot (0-15) or -1 (the default text colour)
local = 14

[[language.rule]]              # a regex rule (Rust regex syntax: linear time)
pattern = '^[A-Z_]+\('
role = "keyword"               # a core role or one declared above
in_section = '^IMAGE\('        # optional: only inside the section that starts at a matching line …
end_section = '^FIELD\('        # … and ends at a matching line (default: the next section header)

[[language.declares]]          # names declared in one place, coloured wherever the file uses them
section = '^PGM_FIELDS'        # the lines after a header line matching this …
name = '^\s*(l[A-Za-z0-9_]*)'  # … and this regex, group 1 is the declared name
role = "local"

[[language.names_from]]        # names read from the user's own files
setting = "headers"            # [ext.<name>] headers = ["/path/*.h"] in the user's config.toml
parser = "c-struct-members"    # c-struct-members | c-negative-defines (built in; never code from the extension)
role = "field"
```

Every problem found is reported at once, naming the field (`kuro ext install` prints them). Unknown keys are errors, so a typo or a
field that would set a colour directly is rejected. Limits: manifest 256 KiB, 2 000 entries per list, 100 rules, 16 roles, patterns 1 KiB.

## Colours

The **only** way an extension colours text is a *role*: one of the core roles (`keyword`, `builtin`, `literal`, `string`, `number`,
`comment`, `key`, `operator`, mapped by the user's `[colors]`) or a role the extension declares under `[language.roles]` with a default
palette slot. Slots are 0-15 or -1 (the default text colour). There is no field for RGB, a 256-colour index or a background, and a slot
outside -1..15 is a validation error. So extension text follows the terminal theme exactly like the core editor.

The user remaps an extension's roles in their own config (`[ext.<name>.colors]`, see [CUSTOMIZATION.md](CUSTOMIZATION.md#extname)); and with
`ui.adaptive_colors` (on by default) kuro asks the terminal for its colours once at start-up and, for roles the user did not map, keeps the
declared slot when it is readable on the theme and otherwise chooses the slot that stands out best, or a style (bold, underline, italic).
It chooses among the 16 slots; it never creates a colour. A terminal that does not answer the query gets the declared slots.

## File detection

For a file, in this order: exact file names and name patterns of extension languages; exact file names of core modes; file types and
`#!` lines of extension languages; file types and `#!` lines of core modes; and **last** an extension's `first_line_regex`. So a content
marker can never take over a file whose type is known (a C file with a banner comment, an `.html` file), and the same file with the extension
removed is plain text again.

## Names from the user's own files (`names_from`)

Some languages refer to names defined elsewhere (ksystem fields are defined in C headers). The extension can only say *which setting* and
*which built-in reader*; the user lists the files:

```toml
# ~/.config/kuro/config.toml
[ext.ksystem]
headers = ["/path/to/ksystem/*.h"]
```

Readers: `c-struct-members` (`TYPE name;` / `TYPE name[N];` inside `typedef struct { … }`), `c-negative-defines` (`#define name -1601`).
Limits: wildcards only in the file name; regular files only; at most 4 000 files, 64 MiB and 100 000 names; read-only, nothing is executed;
a pattern that matches nothing is a `:warnings` entry. The files are read the first time a file of that language is shown.

## How "only kuro loads an extension" is enforced, and its limits

* **Data only.** There is nothing to run: no scripts, no plugins, no network. The manifest is parsed, validated and turned into highlighting
  tables.
* **Sealed installs.** `kuro ext install` copies the files into `~/.local/share/kuro/extensions/<name>/` and records an HMAC-SHA256 over
  every file (path and content) in `registry.toml`, keyed with a random 32-byte secret in `~/.local/state/kuro/ext.key` (mode `0600`,
  created on the first install). At start-up kuro loads an extension only if the registry has it **and** the seal verifies. An edited or
  added file, a copy placed by hand, a copy moved to another user or machine (another key), or a missing key all fail: the extension is
  skipped with a `:warnings` entry and kuro starts normally.
* **No load-from-path.** There is no command, option or environment variable that loads an extension from a path; the loader only reads the
  registry.
* **Crash safety.** Install stages the files in a hidden folder, renames it into place, and writes the registry (which decides what loads)
  last, atomically; removal writes the registry first. A crash leaves the old or the new state; a folder without a registry entry is never
  loaded, `kuro ext list` shows it as `INCOMPLETE` and `kuro ext remove` cleans it.
* **Limits.** The definitions are readable text and the key is in your home directory, so another program running **as you** can read
  them and could forge a seal if it read the key. The seal stops accidental reuse, tampering and extensions that kuro did not install; it is
  not a defence against a determined attacker with your user's access. An extension asks kuro to read files outside its folder only through
  `names_from`, and only the paths *you* wrote in your config.

## The ksystem extension

The `ksystem-kuro-ext` repository (see its README for what the language looks like and which files use it): `kuro ext install https://github.com/ArieqAqila/ksystem-kuro-ext`, then open a
`sct<N>` or `tmp_*` file, or `:setlang ksystem`.
