#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree

first=$(field selectedPath)
"$OVM" key j; sleep 1; down=$(field selectedPath)
expect_true E-03-01 "j moves down" "[[ '$first' != '$down' ]]"
"$OVM" key k; sleep 1; back=$(field selectedPath)
expect_true E-03-01 "k moves back up" "[[ '$back' == '$first' ]]"
"$OVM" key down; sleep 1; arrow_down=$(field selectedPath)
expect_true E-03-01 "Down matches j" "[[ '$arrow_down' == '$down' ]]"
"$OVM" key up; sleep 1

"$OVM" key shift-g; sleep 1; last=$(field selectedPath)
"$OVM" key g; sleep 1; top=$(field selectedPath)
expect_true E-03-02 "G reaches the last row" "[[ '$last' != '$top' ]]"
expect_true E-03-02 "g returns to the first row" "[[ '$top' == '$ROOT_DIR' ]]"

"$OVM" key ctrl-d; sleep 1; paged=$(field selectedPath)
"$OVM" key ctrl-u; sleep 1; unpaged=$(field selectedPath)
expect_true E-03-03 "Ctrl+D pages down" "[[ '$paged' != '$top' ]]"
expect_true E-03-03 "Ctrl+U pages back" "[[ '$unpaged' == '$top' ]]"
"$OVM" key ctrl-d; sleep 1
"$OVM" key ctrl-b; sleep 1
expect E-03-03 "Ctrl+B pages back without a prefix" selectedPath "$top"

"$OVM" key g; sleep 1; "$OVM" key k; sleep 1
expect_true E-03-04 "the cursor stops at the top" "[[ \$(field selectedPath) == '$ROOT_DIR' ]]"
"$OVM" key shift-g; sleep 1; "$OVM" key j; sleep 1
expect_true E-03-04 "the cursor stops at the bottom" "[[ \$(field selectedPath) == '$last' ]]"

"$OVM" key shift-slash; sleep 2
sheet=$(screen_text)
"$OVM" key esc; sleep 2
after_sheet=$(screen_text)
expect_contains E-03-07 "? opens the shortcut reference" "$sheet" "Quick nav"
expect_missing E-03-07 "escape closes it" "$after_sheet" "Quick nav"
expect E-03-07 "and keeps the blade open" open true

"$OVM" key esc; sleep 2
expect E-03-06 "escape with nothing open closes the blade" open false
open_left; focus_tree
"$OVM" key q; sleep 2
expect E-03-05 "q closes the blade" open false
open_left

summary
