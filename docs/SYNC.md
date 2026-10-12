# Live sync

Open the same file in several kuro terminals and they behave like several VS Code tabs of one file: an edit made in
one terminal appears in the others immediately, **before it is saved**. Saving from any terminal writes everything,
and no terminal can lose the others' work, even if a kuro process dies.

It is on by default. Turn it off with `sync.enabled = false`; files above `sync.max_file_mib` (default 5) open
without it (see [CUSTOMIZATION.md](CUSTOMIZATION.md#sync)). The status bar shows `sync` when a terminal is part of a
group, `sync: re-connecting` for a moment after the leader went away, and `local` when the file is edited alone.

## What you get

| Situation | Behaviour |
|---|---|
| Edit in terminal 1 | terminal 2 and 3 show it at once; the file on disk is unchanged, every view shows `[+]` |
| Save (Ctrl+S, `:w`) in terminal 3 | the file is written once with **all** pending edits; every view becomes clean |
| Two terminals type at the same spot | both texts are kept; the order is the same in every terminal |
| Undo / redo | shared and strictly last-in-first-out, like tabs of one file in VS Code: Ctrl+Z in terminal 1 can undo what terminal 2 typed last |
| Cursor and scroll position | stay on the same text when others insert or delete above them. Selections move with the text. Other terminals' cursors are not drawn |
| Read-only SQL file (not connected) | still receives other terminals' edits |
| Quit | asks to save when the *shared* document has unsaved edits, wherever they were typed |

## How it works

There is no separate daemon to install, start or lose. The **first** kuro to open a file becomes its *leader*: it
also serves the others over a Unix socket in `$XDG_RUNTIME_DIR/kuro/` (directory `0700`, socket `0600`, and peers
must have the same user id). The leader is just another editor window; the others are *followers*. All terminals keep
a full copy of the text and send their edits as small operations (insert / delete at a position). The leader gives
every operation a place in one total order and sends it back; concurrent operations are transformed against each
other (operational transformation) so every copy ends up identical. Undo and redo are done by the leader too, which
is what makes them shared.

The leader role is held by an exclusive `flock` on `<key>.lock` (`<key>` = first 16 hex digits of the SHA-256 of the
file's real path). The kernel releases that lock when the process dies for any reason, so a killed leader never
blocks anyone.

## When a process dies

* **A follower dies:** nothing happens to the others. Its unsaved edits were already sent to the leader and are in
  everyone's copy.
* **The leader dies (even `kill -9`):** the followers notice the closed socket, keep their text and any edit that was
  not acknowledged yet, and compete for the lock. The winner becomes the leader, the others reconnect and hand over
  what they have; the new leader waits a short *recovery window* (0.6 s) for them so that no terminal's edit is
  dropped, then carries on. If you press Ctrl+S during those moments the save is repeated automatically once the new
  leader is ready. Undo history restarts after a handover and kuro says so.
* **Every kuro on the file dies:** the leader writes a journal of the unsaved edits to
  `$XDG_STATE_HOME/kuro/recovery/<key>.journal` (default `~/.local/state/kuro/`, mode `0600`) about once a second
  and when it exits cleanly (`:hardundo` throws these unsaved edits away in every terminal). The next kuro that opens the file restores them as unsaved edits and says
  `recovered N unsaved edits from the last session`. Edits typed in the last second before a simultaneous crash of
  **all** processes can be lost.
* **No leader can be found or started** (for instance the runtime directory is unwritable): the terminal says so
  and edits locally; saving still refuses to overwrite a file that changed on disk.

## Saving safely

The leader writes the file to a temporary file in the same folder, `fsync`s it and renames it over the original, so a
crash during a save leaves either the old or the new file, never half of one. Before writing it checks that the file
on disk is still what it last read or wrote. This check also applies to files opened without live sync.

* **The file changed on disk behind kuro's back** (git checkout, another program): with no unsaved edits, all terminals
  reload it and say so. With unsaved edits, kuro keeps your text, flags a conflict and **refuses to save** until you
  decide: `:w!` overwrites the disk version with yours; to take the disk version instead, close and reopen the file.
* **A save fails** (disk full, permissions): every terminal keeps its edits, nothing is marked clean, and the
  message says why.

## Limits

* Files larger than `sync.max_file_mib` open without live sync, and so do files with Windows (CRLF) line endings or
  invalid UTF-8; the status bar says `local` and a message tells you why.
* `:w OTHERNAME` writes a copy and leaves the group on the original file.
* Cursors of other terminals are not shown (they would only add noise for one person editing in several views).
* Only the same user on the same machine can join a group. There is no network mode.
* The undo history is kept in the leader's memory and is not recovered from the journal.

## Shared SQL buffer

A `.sql` file is an ordinary file, so it is shared by the same mechanism: open it in two terminals and both see the
same text; the statement under the cursor that you run is the shared text. Each terminal has its own database
connection and result view. A SQL file stays read-only in a terminal until that terminal is connected; remote edits
still arrive.

## Files and privacy

| Path | Content | Mode |
|---|---|---|
| `$XDG_RUNTIME_DIR/kuro/<key>.sock`, `<key>.lock` | the group's socket and leader lock (falls back to `/tmp/kuro-<uid>/`) | `0600` in a `0700` folder |
| `$XDG_STATE_HOME/kuro/recovery/<key>.journal` | unsaved edits of the shared file (plain text of what you typed) | `0600` |

SQL passwords are never part of any of this: they exist only in memory (see the README) and are not synced.

## Tested by

`src/sync/*` unit tests, a deterministic cluster simulator with random message delivery, kills at every step and
fault-injected saves (`tests/sync_sim.rs`), real sockets / locks / journals in one process (`tests/sync_net.rs`),
the editor with several terminals (`tests/ui_sync.rs`), and separate `kuro` processes in pseudo-terminals including
`SIGKILL` of the leader and of every process (`tests/pty_sync.rs`). See [TESTING.md](TESTING.md).
