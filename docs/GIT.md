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

**What changed inside a line** is marked too: when a removed line is followed by an added line (the first removed with the first added, and so on), the words that
differ are drawn in **reverse video** on both lines, in the line's colour, so for `"20260710"` → `"20260721"` only the `10` and the `21` are reversed: you see where the change is, not only that the line differs.
Whole new or whole removed lines, and pairs that share no word at all, are not marked (the line colour says it all). Never a background colour.

## How it uses git, and why

kuro runs the **`git` command** (read-only subcommands only: `rev-parse`, `symbolic-ref`, `ls-tree`, `rev-list`, `log`, `diff`, `diff-tree`, `show`, `hash-object`; the one exception is
`commit-graph write`, which writes only into kuro's own cache folder, see below) rather than a library:
it gives exactly what your own git gives (your configuration, rename detection, worktrees, huge repositories), and it adds no dependency or C library to kuro.
The cost is that `git` must be installed, and that output is parsed (with NUL-separated, machine-readable formats, so odd file names are safe).

Every call is `git --no-pager -C <repository> …` with `LC_ALL=C`, no terminal prompts and no optional locks (it never blocks or modifies a repository), and every
call that produces a diff passes `--no-ext-diff --no-textconv`: **a repository cannot make kuro run a program it configured** (`diff.external`, `textconv`).

## Speed on large repositories

Finding the commits that touched **one file** is the slow part for git itself: it compares commits with their parents (about 14 seconds with `git log --follow` in a
repository of 108,000 commits). kuro is built around not waiting for that:

* **It does not use `--follow`.** It lists the commits of the path with `git log -- <path>` (which streams, and which a commit graph can speed up) and, where the
  path was *added* in a commit, asks `git diff-tree -M` whether it was renamed from another path; if so the history continues from that commit's parent under the old
  name, which is exactly where `--follow` switches too (a test compares the two on renames, chains, re-created files and merges).
* **The view opens at once** and the commits are added as git prints them, newest first, like `gitk` (header: `reading… N found so far`). You can scroll, open a commit
  or press `Esc` meanwhile; leaving the history for the tree stops git.
* **A private commit graph.** For repositories with more than `git.graph_cache_min_commits` commits (default 5000) kuro builds, in the background, a commit graph
  *with changed-path filters* in its own cache folder, `$XDG_CACHE_HOME/kuro/git/<id>/` (`~/.cache/kuro/git/…`): about 20 seconds of one CPU core and 7 MiB for 100,000
  commits, rebuilt when it is a day old. It is **never inside the repository**: git is told to use that folder as its primary object folder with the repository's own
  objects as an alternate (`GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`), so nothing in the repository changes. Turn it off with `git.graph_cache = false`,
  or delete the folder.

Measured on the 108,000-commit repository, `kuro git -fl FILE` for a file with 51 commits:

| | first commit on screen | 41st commit |
|---|---|---|
| kuro 0.2.0 at first | after about 28 s | after about 28 s |
| now, first time (no cache yet) | 0.9 s | 2.2 s |
| now, with the cache | 0.14 s | 0.35 s |

You can give git the same speed-up for your own `git log -- file` by running `git commit-graph write --reachable --changed-paths` in the repository (that one *does* write
into it).

## Not covered## Not covered

No staging, committing, checkout, blame or branch switching; merges are shown against their first parent only; very long diffs are not paged (they are read whole);
and the terminal tests cannot show how a particular terminal draws the box characters of the scrollbars.
