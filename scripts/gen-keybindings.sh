#!/bin/sh
# Regenerate the keybinding tables inside docs/KEYBINDINGS.md from src/keymap.rs.
set -eu
cd "$(dirname "$0")/.."
doc=docs/KEYBINDINGS.md
begin='<!-- BEGIN KEYBINDINGS (generated: make docs) -->'
end='<!-- END KEYBINDINGS -->'
gen=$(mktemp)
out=$(mktemp)
trap 'rm -f "$gen" "$out"' EXIT
cargo run -q --example keybindings_md > "$gen"
awk -v begin="$begin" -v end="$end" -v gen="$gen" '
  $0 == begin { print; print ""; while ((getline line < gen) > 0) print line; skipping = 1; next }
  $0 == end   { skipping = 0 }
  !skipping   { print }
' "$doc" > "$out"
cp "$out" "$doc"
echo "updated $doc"
