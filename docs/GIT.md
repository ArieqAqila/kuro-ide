# kuro git

A read-only browser for the history of the git repository you are in.

```fish
kuro git                    # the file tree (with each file's history) and the history of the repository
kuro git -fl src/app.rs     # the commit history of one file, renames followed
```

* Not inside a git repository: prints `this directory is not in a git repository` on standard error and exits with status **128** (what `git` itself uses).
  `git` itself not installed: `git is not installed`, status 127. `-fl` with a file that has no history: `no history for <file>`, status 1.
* Works from any folder inside the repository. `-fl FILE` is relative to the current folder (`..` works; the file may have been deleted).
* A repository with no commits opens and says `no commits yet`. A detached HEAD is named in the header (`HEAD detached at 1a2b3c4`).

## Views

1. **Tree**: the files at HEAD, folders folded. `Enter` opens a folder or shows a file's history.
2. **History**: commits, newest first: short hash, date, author, subject. Of the whole repository, or of one file with `--follow`, so a rename does not
   end it (the renaming commit is marked `[R old → new]`). Commits are read 200 at a time as you go down.
3. **Commit**: author, date, the full message, and the files it changed with `A`dded / `M`odified / `D`eleted / `R`enamed (`R old → new`), taken against the
   first parent (a merge shows what it brought in; the root commit is all additions).
4. **Diff**: the unified diff of a commit or of one file in it, with the colours below, vertical and horizontal scrollbars, `n` / `p` to step through commits.

`o` opens a file **as it was at that commit** in the normal editor: syntax colours, find, scrollbars, copying, but **read-only**: every edit and every write
(`Ctrl+S`, `:w`, `:e`, …) is refused, and nothing in the working tree is touched. A file deleted by the commit opens as it was in the parent. `q` closes it.

## Keys

| View | Keys | What |
|---|---|---|
| Everywhere | `q Ctrl+Q` | Quit |
| Everywhere | `Esc Backspace` | Go back one view |
| Lists | `↑ ↓ k j` | Move the selection |
| Lists | `PageUp PageDown` | Move a page |
| Lists | `Home End g G` | First / last row |
| Tree | `Enter →` | Open a folder (or fold it); on a file: show that file's history |
| Tree | `a Tab` | History of the whole repository |
| History | `Enter →` | Open the selected commit |
| History | `d` | Show its diff (of the followed file in a file history) |
| History | `Tab t` | Back to the tree |
| Commit | `Enter →` | Diff of the selected file in this commit |
| Commit | `d` | The whole diff of the commit |
| Commit | `o` | Open the selected file as it was at this commit (read-only) |
| Commit | `n p` | Next older / newer commit |
| Diff | `↑ ↓ PageUp PageDown Home End` | Scroll |
| Diff | `← → h l` | Scroll sideways |
| Diff | `n p` | Next older / newer commit, same file if it is there |
| Diff | `o` | Open the file at this commit (read-only) |
| File view | `q` | Close the file view (Ctrl+Q and :q too) |
| Mouse | `Click a row / double-click` | Select / open |
| Mouse | `Wheel` | Scroll |
| Mouse | `Click or drag a scrollbar` | Step, page, scroll |

(The table is checked against the program by `tests/git_ui.rs`: a key that exists in one and not the other fails the build.) A click selects a row and a
second click on it opens it; the horizontal wheel scrolls a wide diff sideways.

## Colours

Diff colours are roles in `[colors]`, mapped to palette slots like every other colour (no RGB, no 256-colour, no background):
`diff_add` (default 2), `diff_del` (1), `diff_hunk` (6) and `diff_meta` (-1: file headers in bold default text). See [CUSTOMIZATION.md](CUSTOMIZATION.md).
Everything else uses the default text colour, bold, dim and reverse video.

## How it uses git, and why

kuro runs the **`git` command** (read-only subcommands only: `rev-parse`, `symbolic-ref`, `ls-tree`, `log`, `diff`, `show`, `hash-object`) rather than a library:
it gives exactly what your own git gives (your configuration, rename detection, worktrees, huge repositories), and it adds no dependency or C library to kuro.
The cost is that `git` must be installed, and that output is parsed (with NUL-separated, machine-readable formats, so odd file names are safe).

Every call is `git --no-pager -C <repository> …` with `LC_ALL=C`, no terminal prompts and no optional locks (it never blocks or modifies a repository), and every
call that produces a diff passes `--no-ext-diff --no-textconv`: **a repository cannot make kuro run a program it configured** (`diff.external`, `textconv`).

## Speed on large repositories

The history of **one file** is the slow part: git has to compare every commit with its parent to find the ones that touched the file, and to follow
renames it must walk the whole history. In a repository of about 108,000 commits that took git itself about 14 seconds (and kuro used to do it three times;
it now reads the commits and the renames in **one** walk, about 9 seconds there). While it works, kuro says so (`Reading the history of … a large repository
can take a while`) instead of looking frozen; `kuro git -fl FILE` prints the same line on the terminal before the screen opens. The first 200 commits are read
first; going further down reads the history again with a larger limit (git cannot skip commits while it follows renames).

You can make git itself much faster for the plain per-file log (it did 5.6 s → 0.2 s on that repository) by writing a commit graph with changed-path filters, once,
in your own repository (kuro never changes a repository):

```fish
git commit-graph write --reachable --changed-paths
git config fetch.writeCommitGraph true      # keep it up to date after fetches
```

Following renames (`--follow`) does not use those filters, so it stays a full walk.

## Not covered

No staging, committing, checkout, blame or branch switching; merges are shown against their first parent only; very long diffs are not paged (they are read whole);
and the terminal tests cannot show how a particular terminal draws the box characters of the scrollbars.
