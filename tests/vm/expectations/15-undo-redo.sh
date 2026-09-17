#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree
guest "printf 'undoable\n' > $ROOT_DIR/undoable.txt"; sleep 3

click_row undoable.txt
"$OVM" key delete; sleep 2.5; "$OVM" key tab; sleep 1; "$OVM" key ret; sleep 4
label=$(field undoLabel)
expect_contains E-15-01 "the undo label names the operation" "$label" "Trash"

focus_tree
"$OVM" key u; sleep 4
expect_out E-15-02 "u restores the trashed file" "test -f $ROOT_DIR/undoable.txt && echo yes || echo no" yes

focus_tree
"$OVM" key ctrl-shift-z; sleep 4
expect_out E-15-03 "Ctrl+Shift+Z redoes the trash" "test -e $ROOT_DIR/undoable.txt && echo yes || echo no" no
focus_tree
"$OVM" key ctrl-z; sleep 4
expect_out E-15-01 "Ctrl+Z undoes it again" "test -f $ROOT_DIR/undoable.txt && echo yes || echo no" yes

click_row undoable.txt
"$OVM" key r; sleep 2.5; "$OVM" type "renamed-for-undo.txt"; sleep 1.2; "$OVM" key ret; sleep 4
focus_tree
"$OVM" key u; sleep 4
expect_out E-15-04 "undo of a rename restores the old name" "test -f $ROOT_DIR/undoable.txt && echo yes || echo no" yes

click_row undoable.txt
"$OVM" key ctrl-x; sleep 2
click_row dest
"$OVM" key ctrl-v; sleep 4
focus_tree
"$OVM" key u; sleep 4
expect_out E-15-05 "undo of a move returns the file" "test -f $ROOT_DIR/undoable.txt && echo yes || echo no" yes

guest "rm -f $ROOT_DIR/undoable.txt"

ctl clearFolderColor "$ROOT_DIR/deep"; sleep 2
ctl setFolderColor "$ROOT_DIR/deep" "#7aa2f7"; sleep 3
expect_contains E-15-07 "the undo label names the color change" "$(field undoLabel)" "olor"
focus_tree
"$OVM" key u; sleep 4
undone=$("$OVM" ipc "$PLUGIN" folderColor "$ROOT_DIR/deep" 2>/dev/null | jq -r '.color')
expect_true E-15-07 "undo returns the previous color" "[[ -z '$undone' || '$undone' == null ]]"

summary
