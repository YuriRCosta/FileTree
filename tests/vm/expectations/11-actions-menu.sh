#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

menu_text() { ocr_crop ocr-menu 430x620+375+190 200% 6; }
close_menu() { [[ $(field actionMenuOpen) == true ]] && { "$OVM" key esc; wait_for "[[ \$(field actionMenuOpen) == false ]]" 8; }; return 0; }
open_menu_on() { close_menu; click_row "$1" || return 1; "$OVM" key m; wait_for "[[ \$(field actionMenuOpen) == true ]]" 10; }

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree

idx=$(row_index deep)
"$OVM" mouse rclick "$ROW_X" "$(row_y "$idx")"; sleep 2.5
expect E-11-01 "right-click opens the menu" actionMenuOpen true
expect E-11-01 "on the row it was aimed at" actionMenuPath "$ROOT_DIR/deep"
close_menu

open_menu_on deep
expect E-11-02 "m opens the menu for the cursor row" actionMenuOpen true
expect E-11-02 "on the same row" actionMenuPath "$ROOT_DIR/deep"

"$OVM" type "copy"; sleep 2
filtered=$(menu_text)
expect_contains E-11-03 "the filter narrows the rows" "$filtered" "Copy"
expect_missing E-11-03 "and drops what does not match" "$filtered" "Move to Trash"

ctl clearClipboard; sleep 1
"$OVM" key ret; sleep 2.5
expect E-11-04 "Enter runs the first match" clipboardCount 1
expect E-11-04 "and the menu closes" actionMenuOpen false

open_menu_on deep
"$OVM" key down; sleep 1; "$OVM" key down; sleep 1
expect E-11-05 "the menu stays open while moving" actionMenuOpen true

shown=$(menu_text)
expect_contains E-11-06 "Copy advertises its key" "$shown" "Copy y"
expect_contains E-11-06 "Cut advertises its key" "$shown" "Cut x"
expect_contains E-11-06 "Rename advertises its key" "$shown" "F2"
expect_contains E-11-06 "Trash advertises its key" "$shown" "Del"
expect_contains E-11-06 "the editor row advertises its key" "$shown" "LazyVim e"

close_menu
ctl clearFolderColor "$ROOT_DIR/deep"; sleep 1
open_menu_on deep
for _ in $(seq 1 12); do "$OVM" key down >/dev/null; done; sleep 1.5
for _ in 1 2 3 4; do "$OVM" key right >/dev/null; done; sleep 1
"$OVM" key ret; sleep 3
colour=$("$OVM" ipc "$PLUGIN" folderColor "$ROOT_DIR/deep" 2>/dev/null | jq -r '.color')
expect_true E-11-07 "a colour is applied from the keyboard alone" "[[ -n '$colour' && '$colour' != null && '$colour' != '' ]]"

restart_shell
ensure_left_open
ctl focusBlade left; wait_for "[[ \$(field focusedBlade) == left ]]" 12
goto_root "$ROOT_DIR"
wait_for "[[ \$(field treeEntries) -ge 1 ]]" 15
focus_tree
after=$("$OVM" ipc "$PLUGIN" folderColor "$ROOT_DIR/deep" 2>/dev/null | jq -r '.color')
expect_true E-11-08 "the colour survives a shell restart" "[[ '$after' == '$colour' ]]"

open_menu_on deep
"$OVM" key esc; sleep 2
expect E-11-09 "escape closes the menu" actionMenuOpen false
expect E-11-09 "and leaves the blade open" open true
expect E-11-09 "and focused" focusedBlade left

click_row alpha.txt
"$OVM" key r; sleep 2.5
"$OVM" key end; sleep 0.6
"$OVM" type ".bak"; sleep 1.2
"$OVM" key ret; sleep 3
expect_out E-11-10 "the keyboard route seeds the current name" "test -f '$ROOT_DIR/alpha.txt.bak' && echo yes || echo no" yes
guest "mv '$ROOT_DIR/alpha.txt.bak' '$ROOT_DIR/alpha.txt'"; sleep 3

idx=$(row_index alpha.txt)
"$OVM" mouse rclick "$ROW_X" "$(row_y "$idx")"; sleep 2.5
"$OVM" type "rename"; sleep 1.5
"$OVM" key ret; sleep 2.5
"$OVM" key end; sleep 0.6
"$OVM" type ".bak2"; sleep 1.2
"$OVM" key ret; sleep 3
expect_out E-11-10 "and the menu route seeds it the same way" "test -f '$ROOT_DIR/alpha.txt.bak2' && echo yes || echo no" yes
guest "mv '$ROOT_DIR/alpha.txt.bak2' '$ROOT_DIR/alpha.txt'"; sleep 3

click_row deep
"$OVM" key ctrl-shift-n; sleep 2.5
"$OVM" type "fresh"; sleep 1.2
"$OVM" key ret; sleep 3
expect_out E-11-11 "New folder starts empty in the selected directory" "test -d '$ROOT_DIR/deep/fresh' && echo yes || echo no" yes
expect_out E-11-11 "and never seeds the row name" "test -e '$ROOT_DIR/deep/deepfresh' && echo yes || echo no" no
guest "rmdir '$ROOT_DIR/deep/fresh' 2>/dev/null; true"

summary
