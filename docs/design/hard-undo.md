# Design proposal: hard undo (Feature 3)

Status: **proposed, waiting for approval.** Nothing is implemented yet.

## What it is

`:hardundo` restores every file edited in a session to the content it had **before the session started**, even after all
kuro terminals were closed. It is separate from Ctrl+Z (which stays per buffer / shared across live terminals) and from the
sync recovery journal (which keeps *unsaved* edits and is reset by every save, so it cannot know the pre-edit state).

## Decisions (see the end of the conversation: approved with the changes below)

1. **What is a "session"?** Options:
   * **A (recommended): one global session** = from the moment the first kuro starts while no other kuro is running until the
     last one exits. Every file edited by any terminal in that window belongs to it, and `:hardundo` restores all of them.
     Matches "each edited file" and "even after every terminal has been closed". A new session starts the next time kuro is
     started from nothing; older sessions stay available (see retention).
   * B: one session per file (from the file's first edit to the last close of that file). Simpler to explain, but
     `:hardundo` then has to ask *which* file, and "each edited file" turns into a menu.
2. **Which session does `:hardundo` act on?** Recommended: the **latest session that recorded at least one edit** (the
   running one, or the last one that ended). No session picker in this version.
3. **Retention** (below): are 10 sessions / 30 days / 200 MiB acceptable defaults?
4. **A file kuro created** (did not exist at session start): restoring means **deleting** it. Recommended: do it, but list it
   as `will be deleted` in the popup and ask the external-change question like any other file.

## Journal format and location

Directory `$XDG_STATE_HOME/kuro/history/` (default `~/.local/state/kuro/history/`), directory mode `0700`, files `0600`:

```
history/
  sessions/<id>/manifest.jsonl     one JSON object per line, appended, each line ends with a CRC32 of its text
  blobs/<sha256>                   full file contents, written once (content addressed)
  lock                             flock while writing, so concurrent kuro processes cannot interleave lines
```

`<id>` is `<unix-seconds>-<random 8 hex>`. Manifest lines:

| Line | Meaning |
|---|---|
| `{"t":"session","started":…,"kuro":"0.1.0 (commit)"}` | first line |
| `{"t":"file","path":"/abs/real/path","existed":true,"pre":"<sha256>","size":N}` | written **before the first edit** of that file is applied, after the blob it names is on disk |
| `{"t":"edit","path":…,"n":3,"after":"<sha256 of the file kuro last saw/wrote>","at":…}` | after every save, and when a session's last terminal closes; `n` = edit steps recorded so far (the undo steps of the shared document) |
| `{"t":"end","at":…}` | clean end of session (its absence just means a crash or kill; nothing else depends on it) |

**Crash safety.** A blob is written to a temp file in `blobs/`, `fsync`ed, then renamed; only then is the `file` line
appended and `fsync`ed. A reader ignores a last line that is truncated or whose CRC fails, so a crash mid-write loses at
most the line being written, never an earlier one, and a `file` line never refers to a missing blob.
Restores write the target atomically (temp file in the same folder, `fsync`, rename), as saves already do.

**Limits.** Files larger than `history.max_file_mib` (default 5, same as sync), non-UTF-8 files and files matching
`history.exclude` globs are not recorded (kuro says so once). Because the journal holds plain copies of your files, a file with
secrets in it ends up in `~/.local/state/kuro/history/` too: `history.enabled = false` turns it all off, `history.exclude`
(default: `*.env`, `.env*`, `*.pem`, `*.key`, `id_*`) skips such files, and `kuro history clear` deletes everything.

## The flow

`:hardundo` → popup listing the latest session's files:

```
 Hard undo: restore 3 files to before this session?
   src/app.rs            14 changes
   notes.txt              2 changes
   new_file.rs            5 changes   (created in this session: will be deleted)
 [y] restore   [n] cancel
```

* `n` / Esc: nothing at all happens. `y`: each file is restored atomically; the status line says how many.
* **Safety (external-change check).** For every file kuro remembers `after` (the hash of what it last saw or wrote). Before
  restoring, the file on disk is hashed. If it differs, or the file vanished, it is flagged `⚠ changed on disk since the last
  edit` and `y` does **not** restore it: a second popup lists only those files and asks `overwrite them as well? [y/n]`
  (`n` skips them, restores the rest). Nothing is ever restored silently.
* Terminals that still have a restored file open see it as an external change (the existing reload / conflict rules of live
  sync apply: unsaved edits are never overwritten without the existing prompt).
* A file that no longer exists but existed at the start is recreated (after the same flag + confirmation).

## Cleanup: manual only (decided)

kuro **never deletes history by itself and never restores by itself.** Growth is the user's to manage, with commands:

```fish
kuro history list                 # sessions: id, date, files, size on disk
kuro history show <id>            # the files and change counts of one session
kuro history delete <id> [<id>…]  # delete one or several sessions (blobs nothing refers to are removed with them)
kuro history clear                # delete everything (asks first)
```

and in the editor `:hardundo` (latest session), `:hardundo list` (a picker of sessions) and `:hardundo <id>`. The one thing kuro does on
its own is **tell you**: when the store passes `history.warn_mib` (default 200) it shows a single status message at start,
`history uses 340 MiB: kuro history list`. It deletes nothing; `history.warn_mib = 0` silences it.

~~Automatic retention (superseded):~~

At kuro start (and by `kuro history clean`): delete sessions older than `history.max_days` (30) or beyond the newest
`history.max_sessions` (10), then delete blobs no remaining manifest references, then, if the store is still above
`history.max_total_mib` (200), drop the oldest sessions. The running session is never deleted. All four are `[history]`
options documented in CUSTOMIZATION.md (and checked against the code by the existing docs test).

## Tests planned

Restore after every client closed (real processes, SIGKILL included); cancel leaves files byte-identical; external change
produces the warning and the second confirmation; restoring a file that no longer exists; a file created in the session is
deleted; a truncated last manifest line and a flipped CRC byte are ignored; crash between blob and manifest line;
retention and blob garbage collection; excluded, too-large and non-UTF-8 files; two terminals recording at once (lock);
docs check for the `[history]` options and the `:hardundo` binding.
