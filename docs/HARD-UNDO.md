# Hard undo

`:hardundo` puts a file back **as it was before kuro first wrote it**: the original, unedited file. It works after every kuro terminal was closed, after kuro was
killed, and for edits made in several terminals (a file shown in ten terminals has one history, and one hard undo takes all of their saves back at once).
**Unsaved edits are part of it:** `:hardundo` also throws away every edit that has not been saved yet, in **all** terminals that have the file open,
including edits kuro recovered from an earlier session (`recovered N unsaved edits from the last session`). The text becomes the file on disk again, the
recovery journal is emptied, and `Ctrl+Z` does not bring the edits back. If nothing was ever saved by kuro, that is all `:hardundo` does and the popup says
`Hard undo: discard all unsaved edits?`; nothing is written to the file. If both exist, one popup restores the saved changes and discards the unsaved ones.
It is separate from `Ctrl+Z`, which undoes edits step by step inside the editor and is forgotten when kuro closes.

```text
:hardundo        this file
:hardundo all    every file kuro has written (a list; you confirm)
```

```fish
kuro history list                  # the files kuro has a history for, and how much room it takes
kuro history show 3fa2             # one file: the original, every write, outside changes, restores (an id may be shortened)
kuro history delete 3fa2 91c0      # delete one or several histories
kuro history clear                 # delete everything (asks; `--yes` skips the question)
```

## What you see

```text
╭ Hard undo: restore 2 files to before kuro first changed them? ─────────────────╮
│ src/app.rs  14 changes                                                          │
│ notes.txt  2 changes  ⚠ also changed outside kuro                               │
│ new.rs  5 changes  (created by kuro: will be deleted)                           │
│                                                                                 │
│ [y] restore   [n] cancel      files marked ⚠ get a second question              │
╰─────────────────────────────────────────────────────────────────────────────────╯
```

* `n` or `Esc`: nothing at all happens. `y`: every file is put back, atomically (the old or the new file, never half), with its permissions.
* **A "change" is a write by kuro** (a save). The original is the file as it was just before the first one.
* **Safety, a second question.** A file is marked ⚠ when it is not what kuro last saw: it was changed after kuro's last write (`changed since kuro's last write`), it
  vanished, or it was changed outside kuro at some point (`also changed outside kuro`: those changes are undone too, because the original is the original). After `y`
  a second popup lists only those files: `y` overwrites them as well, `n` skips them and restores the rest, `Esc` cancels everything. The disk is looked at again at
  the moment of restoring, so a file that changes while a popup is open is never overwritten without being asked.
* A file kuro **created** (it did not exist before) is **deleted** by a hard undo; the popup says so.
* A file that no longer exists is recreated (after the questions).
* This terminal's buffer shows the restored file (unsaved changes in it, and in every other terminal on the file, are discarded: the popup says so). Other terminals on the same file are reloaded by live sync
  like for any outside change (a terminal with unsaved edits gets the usual conflict message instead).

## Files edited with another editor too

kuro treats saves made elsewhere as part of the same history. While kuro has the file open, live sync notices an outside save and shows it in every terminal; when
kuro next writes the file, or the next time it opens it, a difference from what kuro last saw is recorded as `changed outside kuro`. The original is never moved, so
`:hardundo` still goes back to the very first version, and the ⚠ mark tells you the outside changes go with it.

## Where it lives, and what it never does

`$XDG_STATE_HOME/kuro/history/` (default `~/.local/state/kuro/history/`, mode `0700`):

```text
files/<id>/log        one line per fact:  {json}<TAB><crc32>   baseline, writes, outside changes, restores; appended, never rewritten
blobs/<sha256>        the originals (identical originals are stored once)
lock                  held while recording
```

`<id>` is the first 16 hex digits of the SHA-256 of the file's real path (a symbolic link is the same file).

* **kuro never deletes any of it, and never restores by itself.** There is no automatic clean-up, no expiry and no size limit; growth is yours to manage with
  `kuro history delete` / `clear`. The only thing kuro does on its own is *tell* you: at start-up, when the history is bigger than `history.warn_mib` (200 by default)
  it shows `hard-undo history uses N MiB: kuro history list`. `warn_mib = 0` turns that off.
* **Crash safety.** The original is written to a temporary file, synced and renamed into `blobs/` *before* the log line that names it; that line is synced *before*
  kuro's first write replaces the file. A reader ignores a last line that is torn or whose checksum fails, so a crash loses at most the line being written, never
  an earlier one, and the original is on disk before the file it protects can change. A damaged saved original is detected by its checksum at restore time and the file
  is left alone.
* **Secrets.** The history holds plain copies of your files. A file whose name matches `history.exclude` (by default `*.env`, `.env*`, `*.pem`, `*.key`, `id_*`) is never
  recorded, files over `history.max_file_mib` (5) are not recorded (kuro says so when you save one), and `history.enabled = false` turns it all off. Remember to
  `kuro history delete` the history of a file that held a secret you did not exclude.
* **If the history cannot be written** (full disk, a folder you cannot write), saving still works; the status line says hard undo is off for that file.
* Writes kuro makes through live sync (any terminal saving a shared file), `:w`, `:w NAME` and `:wq` are all recorded.

Settings: [`[history]` in CUSTOMIZATION.md](CUSTOMIZATION.md#history).

## Not covered

Writes made by other programs are only noticed, not recorded (kuro does not watch the whole disk); a file edited by kuro that was larger than `history.max_file_mib`
has no hard undo; and a hard undo restores the *original*, not an intermediate state (use `Ctrl+Z` while the file is open, or `git`, for that).
