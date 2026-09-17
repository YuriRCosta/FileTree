#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"
require_guest
fixture >/dev/null
original=$(field autoHideSearch)
restore() {
  ctl setAutoHideSearch "$original"
  guest "$GUEST_PLUGIN/fileblade blade set right notes" >/dev/null
  ctl focusBlade left
}
trap restore EXIT
open_left
goto_root "$ROOT_DIR/repo"
ctl setAutoHideSearch true
"$OVM" mouse move 800 500 >/dev/null
ctl focusTree
sleep 1
bar_text() { ocr_crop ocr-search-visibility "265x28+${bar_x}+${bar_y}" 400% 7 '0%,100%'; }
bar_x=28
bar_y=65
expect_missing E-32-01 "Files hides the idle search field" "$(bar_text)" "Search"
"$OVM" key slash; sleep 1
expect_contains E-32-01 "slash reveals Files search" "$(bar_text)" "Search"
"$OVM" type tracked
"$OVM" key down; sleep 1
expect_missing E-32-01 "leaving Files search hides the query" "$(bar_text)" "tracked"
expect E-32-01 "the filter remains active" searchQuery tracked
"$OVM" key slash; sleep 1
"$OVM" key end
expect_contains E-32-01 "slash restores the query" "$(bar_text)" "tracked"
"$OVM" key esc; sleep 1
expect E-32-01 "Escape clears the filter" searchQuery ""
expect_missing E-32-01 "Escape hides Files search" "$(bar_text)" "Search"

for module in skills memory hooks mcp git; do
  guest "$GUEST_PLUGIN/fileblade blade set right data-goblin.fileblade-$module/$module" >/dev/null
  ctl openBlade right
  ctl focusBlade right
  bar_x=1598
  bar_y=65
  [[ $module == git ]] && bar_y=220
  sleep 2
  ctl focusBlade right
  expect_missing E-32-01 "$module hides the idle search field" "$(bar_text)" "Filter"
  "$OVM" key slash; sleep 1
  expect_contains E-32-01 "$module reveals search with slash" "$(bar_text)" "Filter"
  "$OVM" type probequery
  "$OVM" key down; sleep 1
  expect_missing E-32-01 "$module hides search on focus loss" "$(bar_text)" "probequery"
  "$OVM" key slash; sleep 1
  "$OVM" key end
  expect_contains E-32-01 "$module preserves its filter" "$(bar_text)" "probequery"
  "$OVM" key esc; sleep 1
  expect_missing E-32-01 "$module hides search after Escape" "$(bar_text)" "Filter"
  ctl setAutoHideSearch false
  sleep 1
  expect_contains E-32-01 "$module shows its cleared search when disabled" "$(bar_text)" "Filter"
  ctl setAutoHideSearch true
done
restart_shell
expect E-32-01 "auto-hide survives restarting the shell" autoHideSearch true
summary
