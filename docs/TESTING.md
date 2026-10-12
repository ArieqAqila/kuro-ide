# Testing

Testing is part of every feature: a feature is not done until its tests pass, and every fixed bug has a
regression test that failed before the fix.

## Running

```fish
make test               # everything below, in order (stops at the first failing group)
make report             # run everything and print passed / failed / ignored / skipped per layer
make test-unit          # fast; run on every change
make test-prop          # property / fuzz, short (256 cases each)
make test-snapshots     # golden screens
make test-ui            # cell-buffer UI tests + the real binary in a pseudo-terminal
make test-integration   # real MariaDB and PostgreSQL in Docker
make test-prop-long     # property / fuzz, long (PROPTEST_CASES, default 20000; nightly CI uses 100000)
make update-snapshots   # regenerate golden screens after an intended change; review the diff
make docs               # regenerate the keybinding tables in docs/KEYBINDINGS.md
make lint               # cargo fmt --check and clippy -D warnings
```

All of these work the same from fish, bash and zsh. `sh scripts/ci.sh` is what CI runs.

## Layers

| Layer | Make target | Test files | What it covers |
|---|---|---|---|
| Unit | `test-unit` | `src/**` (`#[cfg(test)]`), `tests/classify_fixtures.rs`, `tests/docs_check.rs`, `tests/keybindings.rs`, `tests/syntax_langs.rs` (every language mode: keywords, strings, comments, numbers, operators, brackets; HTML/PHP embedded regions) | find, buffer editing and undo, selection, clipboard round-trip, indentation rules, syntax tokenizer (operators, numbers, strings, comments, bracket depth), word suggestions, table layout, input parser (legacy, kitty `CSI u`, mouse, paste, split reads), key specs, SQL statement splitting, error classification on recorded fixtures, config loading |
| Docs checks | `test-unit` | `tests/docs_check.rs`, `tests/keybindings.rs` | every config option in code is in `docs/CUSTOMIZATION.md` (with its default) and the reverse; every key binding in code is in `docs/KEYBINDINGS.md` and the reverse; every binding has a behaviour probe. Each checker is itself tested against deliberately broken docs. |
| Property / fuzz | `test-prop` | `tests/prop.rs` | the input parser never panics and never makes a key from an incomplete sequence; split reads give identical events; the tokenizer, find engine, buffer undo/redo and table layout keep their invariants for arbitrary input |
| Snapshot | `test-snapshots` | `tests/snapshots.rs`, `tests/snapshots/*.txt` | one screen per language mode, the scrollbars at 80x24 and 120x40, and whole rendered screens at fixed sizes: text, style (reverse/bold/dim/underline) and ANSI colour maps |
| UI (cell buffer) | `test-ui` | `tests/ui_render.rs`, `ui_sql.rs`, `ui_sql_run.rs`, `ui_indent.rs`, `ui_modes.rs`, `ui_suggest.rs`, `ui_cursor.rs`, `ui_select_keys.rs`, `ui_mouse_select.rs`, `ui_ctrl_rightclick.rs`, `ui_hscroll.rs`, `ui_scrollbars.rs`, `color_rule.rs` | what is painted and how input changes it: gutter and the line-number regression, find bar, mouse, clipboard, modal and modeless editing, indentation keys, result table, query flow with a fake driver, pickers, suggestions, and the colour rule |
| Pseudo-terminal | `test-ui` | `tests/pty_ui.rs` | the real `kuro` binary in a pty, screen read through a terminal emulator (`vt100`): first paint, no background, find bar top-right, SGR mouse, Ctrl+C/V, quit prompt, the kitty / legacy / silent terminal behaviours, lone Esc, and the raw output scanned for truecolor, 256-colour and background sequences |
| Integration | `test-integration` | `tests/integration_sql.rs`, `tests/pty_sql.rs` | PostgreSQL 16 and MariaDB 11 in throwaway containers: connect, typed results, syntax / runtime / execution errors, wrong password, 200 000-row result with `row_limit`, cancel, server-side timeout, connection loss, NULLs, warnings and notices, and **every snippet executed on both servers**; plus a full SQL session in the real binary |
| Concurrency (simulator) | `test-sync` | `src/sync/*` unit tests, `tests/sync_sim.rs` | operational transformation (every pair of operations converges), the leader's ordering, shared undo, atomic save and conflict rules, journal and recovery, and a deterministic multi-terminal simulator: random delivery order, a leader or follower killed at every step, saves failing half-way, external file changes during a handover. Seeds are fixed, so a failure reproduces |
| Concurrency (real sockets) | `test-sync` | `tests/sync_net.rs`, `tests/ui_sync.rs` | real Unix sockets, `flock` leader election, journal files and atomic saves in a temp dir, several terminals as threads: live edits, concurrent typing, handover, save during a handover, journal after every process died, permissions, the size limit; and the editor on top (cursor and scroll anchoring, shared undo/save/quit, the shared SQL buffer) |
| Concurrency (processes) | `test-sync` | `tests/pty_sync.rs` | separate `kuro` processes in pseudo-terminals on one file: edits reach the other screen, save from either, **`SIGKILL` of the leader** (the other takes over and loses nothing), **`SIGKILL` of every process** (unsaved text restored from the journal) |
| Extensions | `test-unit`, `test-ui` | `src/ext/*` unit tests, `tests/ext_store.rs`, `tests/ext_ksystem.rs`, `tests/pty_ext.rs`, `tests/cli.rs` | manifest validation (every bad field has a message), HMAC against the RFC vectors, install / list / remove / reinstall in temp dirs, an edited, copied or other-user's extension is skipped, crash leftovers, two installs at once, the `kuro ext` command line in the real binary, the ksystem language on invented fixtures (sections, comments, multi-line strings, local / calc / field / index colours), detection order, and the real binary in a pty reading colours from a terminal emulator, including an emulated light and dark theme |
| `kuro git` | `test-unit`, `test-ui` | `src/gitview/cmd.rs` parsers, `tests/git_cmd.rs`, `tests/git_ui.rs`, `tests/pty_git.rs`, `tests/cli.rs` | all on **throw-away repositories created by the tests** (`GIT_CONFIG_GLOBAL=/dev/null`, fixed author and dates, never the user's repositories or git config): outside a repository (exact message and exit status), no commits, detached HEAD, file history across a rename, a commit with added / removed / renamed files, root and merge commits, odd and non-ASCII names, binary files, 450 commits paged, a configured `diff.external` / `textconv` that must not run; the four views, every key (checked against `docs/GIT.md`), mouse, scrollbars, diff colours, the read-only file view (every edit and write refused), snapshots, and the real binary in a pty |
| Hard undo | `test-unit`, `test-ui` | `src/history/*` unit tests, `tests/history.rs`, `tests/ui_hardundo.rs`, `tests/pty_hardundo.rs`, `tests/cli.rs` | the store (baseline recorded once, binary and non-UTF-8 files restored exactly, files kuro created, outside changes, crashes: a torn last line, a flipped checksum, a blob without its line, a damaged original; exclusions, size limit, management, 60 writes from 6 threads), the editor (popup text, `y`, `n`, `Esc`, the second question, a file that changes while a popup is open, a vanished file, unsaved changes, permissions), the real binary: **SIGKILL then a new process**, two terminals on one file with the sync leader saving and the other following a hard undo, history off |
| Opt-in | (manual) | `tests/clipboard_real.rs` | the machine's real clipboard: `env KURO_TEST_REAL_CLIPBOARD=1 cargo test --test clipboard_real` (saves and restores a text clipboard; refuses to run if it holds anything else) |

## Rules every test follows

* **Deterministic.** Terminal sizes are fixed (snapshots use 64×16; others state theirs). Click timing uses an injected
  clock (`ManualClock`), so no test depends on wall-clock time for double-click detection.
* **No fixed sleeps.** Nothing waits "long enough". Waits poll a condition under a timeout (`wait_until`, `wait_for`,
  `wait_idle`); a stuck condition fails with a message instead of hanging. The only intentional delays are the
  application's own, observable ones (the 30 ms lone-Esc timeout and the bounded 500 ms terminal query), and the
  tests that exercise them wait for their *effect*.
* **Isolated.** Tests never read or write your real config, profiles, clipboard or databases. The UI helpers point
  `sql.connections_file` at a path that cannot exist; pty tests get a temporary `KURO_CONFIG_DIR` and have
  `WAYLAND_DISPLAY`/`DISPLAY` removed and the clipboard backend forced to OSC 52; integration tests start their own
  containers bound to `127.0.0.1` on random ports with a random password and remove them afterwards. The only
  database they ever connect to is one they started.
* **No test is deleted, skipped or weakened without a stated reason.** See "Changes to existing tests" below. A test
  that cannot run in an environment prints `SKIPPED …` and is counted as *skipped* in `make report`.
* **Regression tests fail before the fix.** Each bug fix adds a test that was run against the unfixed code first
  (the line-number bug's test printed `4 3 2 1 5 1 2 3 4 5` before the fix).

## Fixtures

| Path | What it is |
|---|---|
| `tests/fixtures/db_errors.txt` | error messages recorded from real servers (`postgres:16-alpine`, `mariadb:11`) by the integration tests with `KURO_RECORD_FIXTURES=<file>`; format `engine|code|expected category|message`. A few hand-written entries for conditions that are awkward to provoke are marked as such. |
| `tests/snapshots/*.txt` | golden screens: characters, a style grid (`R` reverse, `U` underline, `B` bold, `I` italic, `D` dim) and a colour grid (`r g y b m c` ANSI colours, uppercase = bright, `w`/`W` white). `?` would mean an RGB/indexed colour and must never appear. |
| `tests/prop.proptest-regressions` | shrunk inputs of past property-test failures, replayed on every run |

Refreshing the recorded errors: `env KURO_RECORD_FIXTURES=/tmp/rec.txt make test-integration`, then compare
`/tmp/rec.txt` with `tests/fixtures/db_errors.txt`.

## Reporting

`make report` (or `sh scripts/test-report.sh`) runs the whole suite and prints, per layer, the number passed, failed,
ignored and skipped, then exits non-zero on any failure. Run it before every commit and quote it in the report.

## What cannot be tested here

* **The Wayland/X11 clipboard in CI**: only the choice of tool and an in-memory clipboard are tested automatically;
  the real `wl-copy`/`wl-paste` round trip is the opt-in test above (it passed on the development machine, Wayland + foot's session).
* **Which terminals send horizontal wheel events (buttons 6/7) or let Shift+wheel through**: the parser is tested with SGR, X10 and urxvt reports and the editor with the events, but no real terminal's wheel was tried.
* **Mouse and key encodings of a specific terminal** (foot, kitty, WezTerm, GNOME Terminal, tmux, ssh): the pty tests
  emulate a generic xterm with SGR mouse, and a kitty-protocol, legacy or silent terminal for keys.
* **Database versions** other than PostgreSQL 16 and MariaDB 11, TLS connections, MySQL.
* **The OS keyring**: not used. Passwords are kept in process memory only (see the README).
* **Live sync on other platforms or file systems**: tested on Linux with a local file system; `flock` on network file systems and non-Linux systems is not tested. Sync sockets live in the runtime directory, never on the file's file system.
* **Power loss during a save**: the rename is atomic and the file is `fsync`ed, but a real power cut is not simulated; the save-failure paths are tested with injected faults.
* **Very large files and result sets** beyond the 200 000-row case.

## Changes to existing tests (and why)

* `f5_without_a_selection_explains_itself` was removed and replaced by `f5_no_longer_runs_anything`: F5 was removed by the task.
* `status_bar_shows_file_modified_position_and_type` now opens a `.go` file: a `.sql` buffer is read-only without a
  connection (task Part 4.1), so typing into one is no longer the right way to test the modified marker.
  The SQL behaviour is covered in `ui_sql_run.rs`.
* `slow_second_click_is_not_a_double_click` (real `sleep`) was replaced by
  `clicks_within_the_window_are_double_and_slower_ones_are_not` (injected clock), which checks both sides of the boundary.
* Gutter tests were rewritten for absolute numbers by default and a width that follows the digit count (task Part 2).
* The pty harness lost its fixed 40 ms sleep after every key; tests now wait for screen conditions, and send Esc as
  the kitty-protocol form (`CSI 27 u`) when the next key follows immediately, as a real kitty-protocol terminal would.
