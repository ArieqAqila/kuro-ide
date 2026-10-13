# Design proposal: `kuro git` (Major Update 1)

Status: **proposed, waiting for approval.** Nothing is implemented yet.

## Decisions I need from you

1. **`git` command or a Rust git library?** Recommended: **the `git` command.** Same results as the user's own git (config,
   rename detection, worktrees, large repositories), no new dependency or C library in the build, and the binary stays small.
   The cost is that `git` must be installed (it is where a git history browser is useful anyway; without it `kuro git` prints `git is not installed`
   and exits 127) and that output must be parsed. Alternatives: `gix` (pure Rust, large dependency tree and
   compile time) or `git2` (needs libgit2). I would only switch if you want it to work without a `git` binary.
2. **Where does `this directory is not in a git repository` go and what is the exit code?** Recommended: **standard error**, a single line
   with a newline, **exit code 128** (what `git` itself uses outside a repository). Stdout and exit code 1 are equally easy.
3. **Other messages:** only that one is in Indonesian in your spec; I would keep the rest of kuro's UI in English. Please
   say if you want them in Indonesian too.
4. **Scope of the first version:** history only (no staging, committing, checkout, blame). Say if you want `blame` or a
   branch switcher.

## Command line

```fish
kuro git                  # browse the repository that contains the current directory
kuro git -fl src/app.rs   # open the history of one file directly (path relative to the current directory)
```

Not inside a repository (checked with `git rev-parse --show-toplevel`): print `this directory is not in a git repository`, exit non-zero.
`-fl` with a file that has no history in this repository: `no history for <path>` on stderr, exit 1. A repository with no commits opens normally and says
`no commits yet`; a detached HEAD shows `HEAD detached at <short hash>` in the header.

## How git is called (all read-only)

Every call is `git -C <toplevel> --no-pager …` with `LC_ALL=C`, `GIT_OPTIONAL_LOCKS=0`, `-c core.quotepath=off`, paths after
`--`, machine-readable output (`-z`, `--format` with `%x1f` / `%x1e` separators) and **`--no-ext-diff --no-textconv`**: a
repository can configure external diff / textconv programs and kuro must not run anything a repository asks for.

| Need | Command |
|---|---|
| tree at HEAD | `ls-tree -r -z --name-only HEAD` (tracked files; folders are folded) |
| commit list (paged, 200 at a time) | `log --format=… --max-count=200 --skip=N [--follow] [-- path]` |
| files of a commit | `show --name-status -M -z --format= <hash>` (renames as `R old → new`; merges against the first parent) |
| diff of a commit / of one file in it | `show -M --format= <hash> [-- path]` (`-p`, unified, no colour) |
| a file at a commit (read-only) | `show <hash>:<path>` (the old path for renames) |
| file history across renames | `log --follow --name-status -M -- path` |

Binary files show `Binary files differ`; very long diffs are cut at a configurable size with a `… truncated` row.

## Screens and keys

One full-screen view, three panes that you move between (like a history viewer, not a new editor):

1. **Tree** — the repository's files; `Enter` on a file opens its history (also by click).
2. **History** — commits (short hash, date, author, subject); `Enter` opens one. With `-fl` or after picking a file this is
   the file's history (`--follow`, so a rename does not end it); otherwise the whole repository.
3. **Commit** — author, date, full message, changed files (`A` / `M` / `D` / `R`); `Enter` on a file shows its diff, `d` the
   whole diff; `o` opens the file at that commit in the normal editor, **read-only**, with syntax colours and the new scrollbars.

`↑↓ / j k`, `PgUp PgDn`, mouse wheel and scrollbars scroll; `n` / `p` move to the next / previous commit while a diff is open;
`Esc` or `Backspace` goes back; `q` quits. Diff colours are roles in `[colors]` (`diff_add`, `diff_del`, `diff_hunk`,
`diff_meta`) mapped to ANSI palette slots like every other colour: no RGB, no backgrounds (hunk headers bold, removed lines
in the "del" slot). The new keys are registered in `src/keymap.rs`, so the docs check covers them.

## Code layout

`src/gitview/` — `cmd.rs` (runs git, the only place that spawns it), `parse.rs` (pure parsers), `model.rs` + `render.rs` (state
and drawing, reusing the cell-buffer renderer, scrollbars and picker), wired from `main.rs` as a sub-command next to the editor.

## Tests planned

All with throw-away repositories created by the test in a temp dir (`GIT_CONFIG_GLOBAL=/dev/null`, fixed author / dates, never
the user's repositories): outside a repository (exact message and exit code); no commits; detached HEAD; file history across
a rename; a commit with an added, a removed and a renamed file; a merge commit; `-fl` with a missing and with a valid file;
paths with spaces and non-ASCII; binary file; parser unit tests for every git output format with recorded samples;
cell-buffer tests of the three panes and the scrollbars; a pty test of the real binary; a test that kuro refuses to run a
configured `diff.external`.
