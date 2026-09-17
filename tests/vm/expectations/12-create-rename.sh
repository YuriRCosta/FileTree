#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

exists() { guest "test -e '$1' && echo yes || echo no"; }
dialog_do() { "$OVM" key "$1"; sleep 2.5; "$OVM" type "$2"; sleep 1.2; "$OVM" key ret; sleep 3; }

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree

click_row alpha.txt
dialog_do a "made-with-a.txt"
expect_out E-12-01 "a creates a file on disk" "test -f $ROOT_DIR/made-with-a.txt && echo yes || echo no" yes
click_row alpha.txt
dialog_do ctrl-n "made-with-ctrl-n.txt"
expect_out E-12-01 "Ctrl+N creates a file on disk" "test -f $ROOT_DIR/made-with-ctrl-n.txt && echo yes || echo no" yes

click_row alpha.txt
dialog_do ctrl-shift-n "made-folder"
expect_out E-12-02 "Ctrl+Shift+N creates a directory" "test -d $ROOT_DIR/made-folder && echo yes || echo no" yes

click_row bravo.txt
dialog_do r "renamed-with-r.txt"
expect_out E-12-03 "r renames on disk" "test -f $ROOT_DIR/renamed-with-r.txt && echo yes || echo no" yes
expect_out E-12-03 "and the old name is gone" "test -e $ROOT_DIR/bravo.txt && echo yes || echo no" no
dialog_do f2 "renamed-with-f2.txt"
expect_out E-12-03 "F2 renames on disk" "test -f $ROOT_DIR/renamed-with-f2.txt && echo yes || echo no" yes

dialog_do r "alpha.txt"
expect_out E-12-04 "renaming onto an existing name is refused" "test -f $ROOT_DIR/renamed-with-f2.txt && echo yes || echo no" yes
expect_out E-12-04 "and the target is untouched" "cat $ROOT_DIR/alpha.txt" alpha

click_row alpha.txt
dialog_do ctrl-shift-n "bad/name"
expect_true E-12-05 "a name with a slash is refused" "[[ -n \$(field operationError) ]]"
expect_out E-12-05 "and nothing nested is made" "test -e '$ROOT_DIR/bad' && echo yes || echo no" no

for name in ".." "." "../escaped"; do
  click_row alpha.txt
  dialog_do ctrl-shift-n "$name"
done
expect_out E-12-06 "dot names create nothing outside the directory" "test -e /home/omarchy/escaped && echo yes || echo no" no

before_count=$(guest "ls -A $ROOT_DIR | wc -l")
click_row alpha.txt
dialog_do a "   "
"$OVM" key esc; sleep 1
expect_out E-12-07 "a whitespace-only name creates nothing" "ls -A $ROOT_DIR | wc -l" "$before_count"

click_row alpha.txt
"$OVM" key a; sleep 2.5
"$OVM" type "trailing-space "; sleep 1.2
"$OVM" key ret; sleep 3
expect_out E-12-08 "a trailing space keeps that exact name on disk" "test -e '$ROOT_DIR/trailing-space ' && echo yes || echo no" yes

summary
