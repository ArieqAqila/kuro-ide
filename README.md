# kuro

A `vi`-style terminal editor with VS Code-like usability and a built-in SQL client for **PostgreSQL** and **MariaDB**.
It uses your terminal's own colours (no backgrounds, no truecolor), so any theme works.

## Install (fish)

Needs Rust 1.85+ (`pacman -S rust` on Arch). Optional: `wl-clipboard`, `xclip` or `xsel` for the system clipboard.

```fish
git clone https://github.com/ArieqAqila/kuro-ide ~/src/kuro; and cd ~/src/kuro
make install                       # cargo install --path . + fish completions
fish_add_path ~/.cargo/bin         # once, if ~/.cargo/bin is not on your PATH yet
kuro --version                     # prints the git commit it was built from
```

After pulling changes, run `make install` again and restart any running kuro.

Optional, to use it as `vi` without touching the system `vi`:

```fish
alias vi kuro; funcsave vi
# switch back:
functions -e vi; rm ~/.config/fish/functions/vi.fish
```

## Use

```fish
kuro notes.txt
```

| Key | Action |
|---|---|
| Ctrl+S / Ctrl+Q | Save / quit (asks if there are unsaved changes) |
| Ctrl+Z, Ctrl+Y | Undo, redo |
| Ctrl+C / X / V / A | Copy, cut, paste, select all (Alt+C/X/V if your terminal reserves Ctrl) |
| Ctrl+F / Ctrl+H | Find / replace (Enter, Shift+Enter step; Esc closes) |
| Ctrl+Enter | SQL: run the selection or the statement under the cursor (fallback: Alt+Enter) |
| Alt+L / `:runline` | SQL: run the current line |
| Alt+S / `:snippets` | SQL snippets for the connected database |
| `:setlang <mode>` | Set the language mode (`auto` to detect again) |
| `:connect NAME` | Connect with a SQL profile |
| Ctrl+P or `:` | Command line |

Mouse works everywhere (click, drag, double-click a word, triple-click a line, wheel). Ctrl+C is a copy, not an
interrupt. For a VS Code feel without modes set `[editor] modal = false` in the config. The full list of keys is in
[docs/KEYBINDINGS.md](docs/KEYBINDINGS.md).

### Several terminals, one file

Open the same file in several kuro terminals and they stay in step like VS Code tabs: unsaved edits appear everywhere,
undo/redo and save are shared, and nothing is lost if a kuro process dies. Nothing to start. Details and limits:
[docs/SYNC.md](docs/SYNC.md).

### SQL

1. Add a connection profile (kept outside the repo):

   ```fish
   mkdir -p ~/.config/kuro
   cp config/default/connections.example.toml ~/.config/kuro/connections.toml
   $EDITOR ~/.config/kuro/connections.toml
   ```

   Passwords are never stored in the file: use `password = "env:PGPASSWORD"` or `password = "prompt"`. A typed password
   is kept in memory only for the session; `:sql forget [profile]` clears it.

2. In the editor: `:connect local_pg`.
3. Put the cursor in a statement and press **Ctrl+Enter** (or Alt+Enter). `:runbuffer` runs everything, `:cancel`
   cancels. Without a connection a SQL buffer is read-only until you `:connect`.
4. Results appear under the editor; press F4 to scroll them, `e` to expand a column. Errors are classified as syntax,
   runtime or execution errors, with line and column.

Connections are not encrypted (no TLS yet): use local databases or an SSH tunnel.

### Configure

Options live in one TOML file (`config/default/config.toml`). Point `KURO_CONFIG_DIR` at a folder containing your own
`config.toml` to use it. Every option is documented in [docs/CUSTOMIZATION.md](docs/CUSTOMIZATION.md). Invalid
settings give a warning (`:warnings`), never a crash.

## More

[docs/TESTING.md](docs/TESTING.md) (`make test`, `make report`) · [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) ·
[CHANGELOG.md](CHANGELOG.md)
