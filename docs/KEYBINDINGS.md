# Keybindings

Every key, command and mouse gesture kuro understands. The tables are **generated from the code**
(`src/keymap.rs`) and checked by `tests/keybindings.rs`: a binding that exists in code but not here, or the
reverse, fails the build, and every entry has a behaviour test. Regenerate after changing a binding with
`make docs`.

Letters in tables are case-sensitive in Normal mode (`G` is Shift+g). Configurable bindings say so.

## Mouse rules

* **Drag** selects up to the nearest character boundary: past the end of a line selects the whole line including
  trailing spaces and tabs; the right half of a tab or a wide character (cells, not characters) includes it.
* **Ctrl+right-click** on whitespace moves the cursor to the end of the whitespace run. Whitespace is a space or a
  tab; a run is the maximal stretch of those on one line, so mixed `␠⇥␠␠` is one run, and any cell of a tab belongs
  to it. The cursor lands on the next non-whitespace character, or at the end of the line for trailing whitespace
  (in Normal mode, on the last character). On anything else it places the cursor under the mouse. VS Code itself has no
  Ctrl+right-click action (Ctrl+click is go-to-definition; its double-click on whitespace selects the whole run, and
  Ctrl+→ steps over it), so this follows the task's wording and VS Code's idea of a whitespace run.

## Horizontal scrolling with the mouse

kuro understands every common way a terminal reports a sideways wheel:

| What the terminal sends | Reporting mode | Handled |
|---|---|---|
| wheel buttons **6 / 7** (left / right) | SGR (`CSI < 66;x;y M`, `CSI < 67;x;y M`), the mode kuro asks for | yes |
| the same buttons | X10 / normal (`CSI M` + 3 bytes) and urxvt (`CSI 98;x;y M`) | yes |
| Shift + **vertical** wheel (button 4 / 5 with the shift bit) | any of the above | yes, as the fallback |
| Alt + **vertical** wheel (button 4 / 5 with the alt bit) | any of the above | yes, for terminals that keep Shift |

The view stops at the start of the text and at the end of the widest line (tabs counted at their display width).
Which terminals send which form depends on the terminal, the mouse or trackpad and the platform, and was **not
verified per terminal here** (see `docs/TESTING.md`): trackpads and tilt wheels normally produce 6/7 where the terminal
supports them; if yours only sends vertical wheel events, Shift+wheel works unless the terminal keeps Shift for its
own use (many terminals use Shift to bypass mouse reporting, which then never reaches kuro; foot does this, and
Shift+wheel then selects text in the terminal instead). In that case use **Alt+wheel**, which terminals pass on.
`kuro` itself cannot tell which case you are in; try both.

## Which terminals can send Ctrl+Enter

Most terminals send the very same byte for **Enter** and **Ctrl+Enter** (a carriage return), so no program can tell
them apart. The **kitty keyboard protocol** fixes that (it also makes Shift+Tab, a bare Esc, Alt+letter and
Ctrl+Shift+Z unambiguous). kuro does not rely on a list: at startup it asks the terminal (`ui.kitty_keyboard = "auto"`)
and tells you in a SQL buffer when the answer is no.

| Terminal | Kitty keyboard protocol |
|---|---|
| kitty, foot, Ghostty, Rio | yes |
| WezTerm | yes, if `enable_kitty_keyboard = true` is set in its config |
| Alacritty 0.13+ | yes |
| iTerm2 | yes (CSI u mode) |
| GNOME Terminal and other VTE terminals, Konsole, xterm, st, Windows Terminal, tmux and screen | no, or not passed through |

This table reflects what each project documents as far as known when it was written; verify with your own terminal,
since kuro's startup check is the authority. Without the protocol:

* Ctrl+Enter arrives as a plain Enter. kuro shows `Ctrl+Enter unavailable (no kitty keyboard protocol): use Alt+Enter or :run`.
* Use the **fallback key** (default `Alt+Enter`, change it with `sql.run_fallback_key`), or `:run`.
* Some terminals send a line feed (`Ctrl+J`) for Ctrl+Enter; kuro treats that as Ctrl+Enter.
* `ui.kitty_keyboard = "on"` skips the query and assumes support; `"off"` never uses the protocol.

`Alt+L` / `:runline` runs only the line under the cursor (change the key with `sql.run_line_key`).
**F5 no longer runs anything**; F6 runs the whole buffer and F7 cancels.

## Tables

<!-- BEGIN KEYBINDINGS (generated: make docs) -->

### Everywhere

| Keys | Action |
|---|---|
| `Ctrl+S` | Save (asks for a name if the buffer has none) |
| `Ctrl+Q` | Quit; asks to save when there are unsaved changes |
| `Ctrl+F` | Open the find bar (top-right) |
| `Ctrl+H` | Open find and replace |
| `Ctrl+Z` | Undo |
| `Ctrl+Y` | Redo |
| `Ctrl+Shift+Z` | Redo (needs the kitty keyboard protocol) |
| `Ctrl+C` | Copy the selection (no selection: the current line) |
| `Ctrl+X` | Cut the selection (no selection: the current line) |
| `Ctrl+V` | Paste |
| `Ctrl+A` | Select all |
| `Alt+C` | Copy (for terminals that reserve Ctrl+C) |
| `Alt+X` | Cut (for terminals that reserve Ctrl+X) |
| `Alt+V` | Paste (for terminals that reserve Ctrl+V) |
| `Ctrl+P` | Open the command line (like `:`; works in Insert mode) |
| `Alt+S` | Open the SQL snippet picker |
| `F3` | Next match of the last search |
| `Shift+F3` | Previous match of the last search |
| `F4` | Focus the SQL result table (and back) |
| `F6` | SQL: run the whole buffer |
| `F7` | SQL: cancel the running query |
| `Ctrl+Enter` | SQL: run the selection, or the statement under the cursor |
| `Alt+Enter` | SQL: same as Ctrl+Enter for terminals that cannot send it (`sql.run_fallback_key`) |
| `Alt+L` | SQL: run only the current line (`sql.run_line_key`) |

### Normal

| Keys | Action |
|---|---|
| `h` | Move left (also ←) |
| `j` | Move down (also ↓) |
| `k` | Move up (also ↑) |
| `l` | Move right (also →) |
| `w` | Next word start |
| `b` | Previous word start |
| `e` | End of word |
| `0` | Start of line |
| `^` | First non-blank of the line |
| `$` | End of line |
| `gg` | First line (`5gg`: line 5) |
| `G` | Last line (`5G`: line 5) |
| `i` | Insert before the cursor |
| `a` | Insert after the cursor |
| `I` | Insert at the first non-blank |
| `A` | Insert at the end of the line |
| `o` | New line below, enter Insert mode |
| `O` | New line above, enter Insert mode |
| `x` | Delete the character under the cursor (`3x`) |
| `X` | Delete the character before the cursor |
| `dd` | Delete line(s) (`3dd`) |
| `yy` | Yank line(s) |
| `cc` | Change line(s) |
| `d{motion}` | Delete over a motion (`dw`, `d$`, `dG`…) |
| `y{motion}` | Yank over a motion |
| `c{motion}` | Change over a motion (`cw` changes to the end of the word) |
| `D` | Delete to the end of the line |
| `C` | Change to the end of the line |
| `Y` | Yank the line |
| `p` | Paste after |
| `P` | Paste before |
| `u` | Undo |
| `Ctrl+R` | Redo |
| `v` | Visual mode (characters) |
| `V` | Visual mode (lines) |
| `:` | Command line |
| `/` | Open the find bar |
| `n` | Next match |
| `N` | Previous match |
| `Ctrl+D` | Half page down |
| `Ctrl+U` | Half page up |
| `1-9` | Count prefix (`5j`, `3dd`) |
| `Shift+← Shift+→ Shift+↑ Shift+↓` | Start or extend a selection (as in Insert mode) |
| `Shift+Home Shift+End` | Select to the start / end of the line |
| `Shift+PageUp Shift+PageDown` | Select a page up / down |
| `Shift+Ctrl+← Shift+Ctrl+→` | Select by word |
| `Esc` | Cancel a pending command, clear the selection, close the find bar |

### Visual

| Keys | Action |
|---|---|
| `h j k l w b e 0 $ gg G` | Extend the selection |
| `d` | Delete the selection (also x) |
| `y` | Yank the selection |
| `c` | Change the selection |
| `p` | Replace the selection with the last yank |
| `o` | Jump to the other end of the selection |
| `v / V` | Toggle character / line selection; leave Visual mode |
| `Esc` | Leave Visual mode |

### Insert

| Keys | Action |
|---|---|
| `Esc` | Leave Insert mode (modal) / clear the selection |
| `Enter` | New line, keeping the indentation |
| `Tab` | Insert an indent; with a selection indent the lines |
| `Shift+Tab` | Outdent the current line or the selected lines |
| `Backspace` | Delete back; in indentation one indent step; on a whitespace run the whole run |
| `Delete` | Delete forward; on whitespace the whole run |
| `Alt+Backspace` | Delete the previous word |
| `← ↑ → ↓` | Move (with Shift: select) |
| `Ctrl+← Ctrl+→` | Move by word (with Shift: select) |
| `Home End` | Line start (first non-blank first) / line end |
| `Ctrl+Home Ctrl+End` | Start / end of the file |
| `PageUp PageDown` | Move a page |

### Find bar

| Keys | Action |
|---|---|
| `Enter` | Next match (in the replace field: replace this one) |
| `Shift+Enter` | Previous match |
| `Esc` | Close the find bar |
| `Alt+C` | Toggle Match case |
| `Alt+W` | Toggle Whole word |
| `Alt+R` | Toggle Regex |
| `Tab` | Switch between the find and replace fields |
| `Alt+A` | Replace all (one undo step) |
| `↑ ↓` | Previous / next match |
| `Ctrl+U` | Clear the field before the cursor |

### Result table

| Keys | Action |
|---|---|
| `j k ↓ ↑` | Scroll one row |
| `PageUp PageDown` | Scroll a page |
| `g G` | First / last row |
| `h l ← →` | Select the previous / next column |
| `Home End` | First / last column |
| `H L Ctrl+← Ctrl+→` | Scroll sideways by 8 cells |
| `e Enter Space` | Expand / collapse the current column |
| `Esc Tab` | Back to the editor |
| `q` | Hide the result pane |
| `Ctrl+C` | Copy the result as TSV |

### Picker

| Keys | Action |
|---|---|
| `↑ ↓ Ctrl+N Ctrl+P` | Move the selection |
| `Enter Tab` | Choose the highlighted item |
| `Esc` | Cancel |
| `(typing)` | Filter the list |

### Suggestions

| Keys | Action |
|---|---|
| `↑ ↓ Ctrl+N Ctrl+P` | Move the selection |
| `Tab Enter` | Accept: replaces the typed prefix |
| `Esc` | Dismiss (stays in Insert mode) |

### Command line

| Keys | Action |
|---|---|
| `:w [file]` | Write |
| `:q  :q!` | Quit (`!` discards changes) |
| `:wq  :x` | Write and quit |
| `:e file` | Open a file |
| `:N` | Go to line N |
| `:hardundo [all]` | Put this file (or every recorded file) back as it was before kuro first wrote it, and discard unsaved edits in every terminal; asks first, and again for files changed outside kuro |
| `:setlang [mode]` | Set the language mode (picker without an argument; `auto` to detect) |
| `:snippets` | Open the SQL snippet picker |
| `:connect NAME` | Connect with a profile from connections.toml |
| `:disconnect` | Disconnect |
| `:run` | Run the selection / current statement |
| `:runline` | Run the current line |
| `:runbuffer` | Run the whole buffer |
| `:cancel` | Cancel the running query |
| `:sql close / show / profiles / warnings / forget` | Result pane, profile list, warnings, and forgetting remembered passwords |
| `:warnings` | Show configuration warnings |

### Mouse

| Keys | Action |
|---|---|
| `Click` | Place the cursor |
| `Drag` | Select up to the nearest character boundary: past the end of a line selects through its trailing spaces and tabs, and the right half of a tab or wide character includes it |
| `Ctrl+right-click` | On spaces/tabs: move the cursor to the end of that whitespace run; elsewhere: place the cursor |
| `Click / drag a scrollbar` | Arrow: step one line or column; track: page; thumb: drag to scroll (bars hide when the text fits; `editor.scrollbars`) |
| `Double-click` | Select the word |
| `Triple-click` | Select the line |
| `Click the line number` | Select the whole line |
| `Wheel` | Scroll up / down |
| `Horizontal wheel, Shift+wheel or Alt+wheel` | Scroll sideways, between the start and the end of the longest line |
| `Click find-bar buttons` | Aa / ab / .* toggles, ↑ ↓, ✕, ↵ replace, ⇉ replace all |
| `Wheel over the table` | Scroll rows (Shift+wheel or Alt+wheel: columns) |
| `Click / double-click a column header` | Select / expand the column |
| `Click a picker row` | Choose it |

<!-- END KEYBINDINGS -->

## Notes

* **Ctrl+C is a copy, never an interrupt.** kuro puts the terminal in raw mode, so Ctrl+C reaches the editor as a
  key. If your terminal emulator or multiplexer reserves Ctrl+C / Ctrl+X / Ctrl+V, use Alt+C / Alt+X / Alt+V, or select
  with the mouse and use the terminal's own copy.
* **Esc** is also the first byte of every escape sequence, so a bare Esc in a terminal without the kitty protocol is
  recognised after a ~30 ms pause. With the protocol it is immediate.
* **Alt+letter** keys need the terminal to send Alt as an Escape prefix (the default almost everywhere) or the kitty protocol.
* In an SQL buffer with no connection, edits and Insert mode are refused (`sql.block_editing_when_disconnected`).
