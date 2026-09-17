#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

expanded_of() { "$OVM" ipc "$PLUGIN" tree 300 2>/dev/null | jq -r --arg n "$1" '[.entries[]?|select(.name==$n)][0].expanded'; }
clients() { "$OVM" hypr clients 2>/dev/null | jq length; }
classes() { "$OVM" hypr clients 2>/dev/null | jq -r '[.[]?|.class]|join(",")'; }
kill_windows() { "$OVM" ssh 'pkill -x nvim; pkill -x foot; pkill -x nautilus' >/dev/null 2>&1; wait_for "[[ \$(clients) == 0 ]]" 15; }

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree
kill_windows

click_row deep
"$OVM" key ret; sleep 3
expect_true E-23-01 "Enter on a folder expands it" "[[ \$(expanded_of deep) == true ]]"
expect E-23-01 "and keeps the current tree root" rootPath "$ROOT_DIR"
expect_out E-23-01 "and does not launch a window" "hyprctl -j clients | jq length" 0
"$OVM" key z; "$OVM" key c; sleep 2

"$OVM" key o; sleep 3
expect E-23-01 "o enters the selected folder" rootPath "$ROOT_DIR/deep"
expect_contains E-23-01 "and shows its contents" "$(tree_names)" "inner"
expect_out E-23-01 "without launching a file manager" "hyprctl -j clients | jq length" 0
"$OVM" key alt-left; sleep 2
expect E-23-01 "Back returns to the previous root" rootPath "$ROOT_DIR"

for pair in "l h" "right left"; do
  read -r enter_key parent_key <<< "$pair"
  click_row deep
  "$OVM" key "$enter_key"; sleep 2
  expect E-23-01 "$enter_key enters the selected folder" rootPath "$ROOT_DIR/deep"
  "$OVM" key "$parent_key"; sleep 2
  expect E-23-01 "$parent_key goes to its parent" rootPath "$ROOT_DIR"
done

click_row long.txt
"$OVM" key ret; sleep 6
wait_for "[[ \$(clients) -ge 1 ]]" 20
expect_true E-23-01 "Enter on a file opens its default application" "[[ \$(clients) -ge 1 ]]"
kill_windows

click_row long.txt
"$OVM" key o; sleep 6
wait_for "[[ \$(clients) -ge 1 ]]" 20
expect_true E-23-01 "o on a file still opens its default application" "[[ \$(clients) -ge 1 ]]"
kill_windows

ctl focusProperties; sleep 2
"$OVM" key o; sleep 6
wait_for "[[ \$(clients) -ge 1 ]]" 20
expect_true E-23-01 "o in Properties keeps opening the selected file" "[[ \$(clients) -ge 1 ]]"
kill_windows

click_row long.txt
"$OVM" key e; sleep 6
wait_for "[[ \$(classes) == *nvim* ]]" 25
expect_contains E-23-03 "e opens the editor" "$(classes)" "nvim"
kill_windows

click_row long.txt
"$OVM" key shift-ret; sleep 3
expect E-23-02 "Shift+Enter offers compatible applications" actionMenuMode "open-with"
expect_true E-23-02 "and lists at least one" "[[ \$(field applicationsCount) -ge 1 ]]"
"$OVM" key esc; sleep 2

ctl focusProperties; sleep 2
"$OVM" key e; sleep 6
wait_for "[[ \$(classes) == *nvim* ]]" 25
expect_contains E-23-04 "e from Properties opens the same editor" "$(classes)" "nvim"
kill_windows
ctl focusTree; sleep 1

ctl select "$ROOT_DIR/long.txt"; sleep 2
ctl focusProperties; sleep 2
"$OVM" key r; sleep 6
revealed=$(field lastLaunchedPath)
expect_true E-23-05 "r from Properties reveals the item" "[[ '$revealed' == *long.txt* || \$(clients) -ge 1 ]]"
kill_windows
ctl focusTree; sleep 1

before_root=$(field rootPath)
ctl setRoot "$ROOT_DIR/deep"; sleep 3
expect E-23-06 "a valid location opens" rootPath "$ROOT_DIR/deep"
ctl setRoot "$ROOT_DIR/nowhere-at-all"; sleep 3
expect E-23-06 "an invalid location does not move me" rootPath "$ROOT_DIR/deep"
expect_true E-23-06 "and reports the problem" "[[ -n \$(field locationValidationError) ]]"
ctl clearLocationError >/dev/null 2>&1
goto_root "$ROOT_DIR"

ctl openPath "$ROOT_DIR/long.txt"; sleep 5
kill_windows
ctl navigate "recent:///"; sleep 4
expect E-23-07 "Recent opens" recentMode true
expect_true E-23-08 "and holds what was opened" "[[ \$(field recentCount) -ge 1 ]]"
expect_contains E-23-08 "including the file just opened" "$(tree_text)" "long.txt"
ctl focusTree
"$OVM" key home
"$OVM" key ret
wait_for "[[ \$(clients) -ge 1 ]]" 20
expect_true E-23-08 "Enter opens the Recent row rather than the search model" "[[ \$(clients) -ge 1 ]]"
kill_windows
goto_root "$ROOT_DIR"

if ! wait_for '[[ $(field launchBusy) == false ]]' 20; then
  fail harness "clear Recent" "the previous launch is still recording its result"
  summary
fi
sleep 1
guest "rm -f /home/omarchy/.local/state/omarchy/fileblade/frecency.json /home/omarchy/.local/share/recently-used.xbel" >/dev/null 2>&1
ctl navigate "recent:///"; sleep 4
expect E-23-09 "precondition: both Recent sources are empty" recentCount 0
expect_contains E-23-09 "an empty Recent says so" "$(tree_text)" "o recent"
goto_root "$ROOT_DIR"

focus_tree
"$OVM" key ctrl-p; sleep 3
expect E-23-10 "Ctrl+P opens the picker" quickNavActive true
picker=
wait_for 'picker=$(picker_text) && [[ $picker == *"> actions"* && $picker == *"~ recent"* && $picker == *"? cont"* ]]' 8
expect_contains E-23-10 "and shows its action prefix" "$picker" "> actions"
expect_contains E-23-10 "and shows its Recent prefix" "$picker" "~ recent"
expect_contains E-23-10 "and shows its content-search prefix" "$picker" "? cont"
"$OVM" key esc; sleep 2


kill_windows
goto_root "$ROOT_DIR"
ctl collapsePath "$ROOT_DIR/deep" >/dev/null; sleep 1.5
double_click_row() {
  click_row "$1"
  double_click "$ROW_X" "$CLICK_ROW_Y"
}
double_click_row deep; sleep 3
expect E-23-11 "double-clicking a folder opens it as the tree root" rootPath "$ROOT_DIR/deep"
expect_contains E-23-11 "and shows its contents" "$(tree_names)" "inner"
expect_out E-23-11 "without launching an external file manager" "hyprctl -j clients | jq length" 0
"$OVM" shot folder-double-click-open >/dev/null
goto_root "$ROOT_DIR"
ctl collapsePath "$ROOT_DIR/deep" >/dev/null; sleep 1.5
click_row deep
"$OVM" key ret; sleep 3
expect_true E-23-11 "an expanded folder is ready for double-click" "[[ \$(expanded_of deep) == true ]]"
double_click "$ROW_X" "$CLICK_ROW_Y"; sleep 3
expect E-23-11 "double-clicking an expanded folder also opens it" rootPath "$ROOT_DIR/deep"
goto_root "$ROOT_DIR"
double_click_row long.txt; sleep 6
wait_for "[[ \$(clients) -ge 1 ]]" 20
expect_true E-23-11 "double-clicking a file opens its application" "[[ \$(clients) -ge 1 ]]"
kill_windows

pending E-23-12 "a companion module opens files with the desktop default application" "requires the dedicated companion fixture; not exercised by this core-opening section"

summary
