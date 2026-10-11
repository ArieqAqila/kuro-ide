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
| Unit | `test-unit` | `src/**` (`#[cfg(test)]`), `tests/classify_fixtures.rs`, `tests/docs_check.rs`, `tests/keybindings.rs` | find, buffer editing and undo, selection, clipboard round-trip, indentation rules, syntax tokenizer (operators, numbers, strings, comments, bracket depth), word suggestions, table layout, input parser (legacy, kitty `CSI u`, mouse, paste, split reads), key specs, SQL statement splitting, error classification on recorded fixtures, config loading |
| Docs checks | `test-unit` | `tests/docs_check.rs`, `tests/keybindings.rs` | every config option in code is in `docs/CUSTOMIZATION.md` (with its default) and the reverse; every key binding in code is in `docs/KEYBINDINGS.md` and the reverse; every binding has a behaviour probe. Each checker is itself tested against deliberately broken docs. |
| Property / fuzz | `test-prop` | `tests/prop.rs` | the input parser never panics and never makes a key from an incomplete sequence; split reads give identical events; the tokenizer, find engine, buffer undo/redo and table layout keep their invariants for arbitrary input |
| Snapshot | `test-snapshots` | `tests/snapshots.rs`, `tests/snapshots/*.txt` | whole rendered screens at fixed sizes: text, style (reverse/bold/dim/underline) and ANSI colour maps |
| UI (cell buffer) | `test-ui` | `tests/ui_render.rs`, `ui_sql.rs`, `ui_sql_run.rs`, `ui_indent.rs`, `ui_modes.rs`, `ui_suggest.rs`, `color_rule.rs` | what is painted and how input changes it: gutter and the line-number regression, find bar, mouse, clipboard, modal and modeless editing, indentation keys, result table, query flow with a fake driver, pickers, suggestions, and the colour rule |
| Pseudo-terminal | `test-ui` | `tests/pty_ui.rs` | the real `kuro` binary in a pty, screen read through a terminal emulator (`vt100`): first paint, no background, find bar top-right, SGR mouse, Ctrl+C/V, quit prompt, the kitty / legacy / silent terminal behaviours, lone Esc, and the raw output scanned for truecolor, 256-colour and background sequences |
| Integration | `test-integration` | `tests/integration_sql.rs`, `tests/pty_sql.rs` | PostgreSQL 16 and MariaDB 11 in throwaway containers: connect, typed results, syntax / runtime / execution errors, wrong password, 200 000-row result with `row_limit`, cancel, server-side timeout, connection loss, NULLs, warnings and notices, and **every snippet executed on both servers**; plus a full SQL session in the real binary |
| Opt-in | (manual) | `tests/clipboard_real.rs` | the machine's real clipboard: `env KURO_TEST_REAL_CLIPBOARD=1 cargo test --test clipboard_real` (saves and restores a text clipboard; refuses to run if it holds anything else) |

### Not implemented yet (waiting for design approval)

* **Concurrency tests** (two or more kuro clients against one daemon) and the shared SQL buffer: they need the live-sync
  design in Part 3.3 of the task, which has been proposed but not approved.
* **Password-cache tests** (reuse on reconnect, wrong password clears, cleared on exit, never on disk): they need the
  password-storage design in Part 4.6, also proposed and not approved.

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
* **Mouse and key encodings of a specific terminal** (foot, kitty, WezTerm, GNOME Terminal, tmux, ssh): the pty tests
  emulate a generic xterm with SGR mouse, and a kitty-protocol, legacy or silent terminal for keys.
* **Database versions** other than PostgreSQL 16 and MariaDB 11, TLS connections, MySQL.
* **The OS keyring** (design for password storage pending approval).
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
