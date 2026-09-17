#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree

expect E-05-01 "a click selects exactly one row" selectedCount 1

"$OVM" key v; sleep 1
"$OVM" key j; sleep 1
"$OVM" key j; sleep 1
expect_true E-05-02 "visual select extends with j" "[[ \$(field selectedCount) -ge 3 ]]"

"$OVM" key esc; sleep 1
expect E-05-03 "escape leaves visual select" open true
expect_true E-05-03 "and the selection collapses" "[[ \$(field selectedCount) -ge 1 ]]"

focus_tree
"$OVM" key ctrl-a; sleep 2
rows=$("$OVM" ipc "$PLUGIN" tree 200 2>/dev/null | jq '[.entries[]?|select(.depth==1)]|length')
expect_true E-05-04 "Ctrl+A selects every row" "[[ \$(field selectedCount) -ge $rows ]]"

focus_tree
one=$(field selectedCount)
"$OVM" key ctrl-spc; sleep 1.5
off=$(field selectedCount)
expect_true E-05-05 "Ctrl+Space takes the cursor row out" "[[ $off -lt $one ]]"
"$OVM" key ctrl-spc; sleep 1.5
expect_true E-05-05 "and puts it back" "[[ \$(field selectedCount) -gt $off ]]"

focus_tree
click_row alpha.txt
expect E-05-06 "the properties pane follows the selection" selectedPath "$ROOT_DIR/alpha.txt"

focus_tree
"$OVM" key v; sleep 1; "$OVM" key j; sleep 1
"$OVM" key m; sleep 2
expect E-05-07 "the menu opens for the multi selection" actionMenuOpen true
expect_true E-05-07 "and reports more than one target" "[[ \$(status | jq '.actionMenuPaths|length') -ge 2 ]]"
"$OVM" key esc; sleep 1
"$OVM" key esc; sleep 1
open_left

summary
