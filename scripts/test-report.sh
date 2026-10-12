#!/bin/sh
# Run every test target and print passed / failed / ignored / skipped per layer.
# "skipped" counts tests that ran but reported SKIPPED (e.g. Docker unavailable); "ignored" is #[ignore].
# Exit status is non-zero if any test failed.
set -u
cd "$(dirname "$0")/.."

layer_of() {
  case "$1" in
    lib|classify_fixtures|docs_check|keybindings|cli) echo "unit" ;;
    prop) echo "property/fuzz" ;;
    snapshots) echo "snapshot" ;;
    ui_render|ui_sql|ui_sql_run|ui_indent|ui_modes|ui_suggest|ui_password|ui_cursor|ui_select_keys|ui_mouse_select|ui_ctrl_rightclick|ui_hscroll|ui_scrollbars|color_rule) echo "ui (cell buffer)" ;;
    sync_sim) echo "concurrency (simulator)" ;;
    sync_net|ui_sync) echo "concurrency (real sockets)" ;;
    pty_ui) echo "pty (real binary)" ;;
    pty_sync) echo "concurrency (processes)" ;;
    integration_sql|pty_sql) echo "integration (docker)" ;;
    clipboard_real) echo "opt-in (real clipboard)" ;;
    *) echo "other" ;;
  esac
}

log=$(mktemp)
trap 'rm -f "$log"' EXIT
cargo test --no-fail-fast -- --nocapture --test-threads=2 > "$log" 2>&1
status=$?

awk -v OFS='\t' '
  /^     Running unittests src\/lib.rs/ { target = "lib"; next }
  /^     Running unittests src\/main.rs/ { target = "main"; next }
  /^     Running tests\// { t = $2; sub(/^tests\//, "", t); sub(/\.rs$/, "", t); target = t; next }
  /^   Doc-tests/ { target = "doc"; next }
  /^test result:/ { print target, $4, $6, $8; next }
  /SKIPPED/ { skipped[target]++ }
  END { for (t in skipped) print t, "SKIP", skipped[t], 0 }
' "$log" > "$log.rows"

# aggregate per layer (rows with no tests, like the binary itself and doc-tests, are left out)
printf "%-26s %7s %7s %8s %8s\n" "layer" "passed" "failed" "ignored" "skipped"
while IFS="$(printf "\t")" read -r target a b c; do
  l=$(layer_of "$target")
  if [ "$a" = "SKIP" ]; then
    echo "$l	0	0	0	$b"
  else
    echo "$l	$a	$b	$c	0"
  fi
done < "$log.rows" | awk -F'\t' '
  { p[$1] += $2; f[$1] += $3; i[$1] += $4; s[$1] += $5; seen[$1] = 1 }
  END {
    for (l in seen) {
      tp += p[l]; tf += f[l]; ti += i[l]; ts += s[l]
      if (p[l] + f[l] + i[l] + s[l] > 0) printf "%-26s %7d %7d %8d %8d\n", l, p[l], f[l], i[l], s[l]
    }
    printf "zzzz%-26s %7d %7d %8d %8d\n", "TOTAL", tp, tf, ti, ts
  }' | sort | sed 's/^zzzz//'
rm -f "$log.rows"

if [ "$status" -ne 0 ]; then
  echo
  echo "FAILED: see the failing tests below"
  grep -E "^test .* FAILED|^---- .* stdout ----" "$log" | head -40
fi
exit "$status"
