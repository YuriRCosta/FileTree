#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree

click_row alpha.txt
"$OVM" key a; sleep 2.5; "$OVM" type "persisted.txt"; sleep 1.2; "$OVM" key ret; sleep 3
expect_out E-18-01 "the new file is on disk at once" "test -f $ROOT_DIR/persisted.txt && echo yes || echo no" yes

ctl close; sleep 3
expect E-18-02 "the blade closes" open false
ctl open; sleep 4
expect E-18-02 "reopening keeps the root" rootPath "$ROOT_DIR"
expect_contains E-18-02 "and lists the new file" "$(tree_names)" "persisted.txt"

ctl setFolderColor "$ROOT_DIR/deep" "#e0af68"; sleep 2
ctl toggleFavorite "$ROOT_DIR/dest"; sleep 2
restart_shell
open_left; goto_root "$ROOT_DIR"
expect E-18-03 "the root survives a shell restart" rootPath "$ROOT_DIR"
colour=$("$OVM" ipc "$PLUGIN" folderColor "$ROOT_DIR/deep" 2>/dev/null | jq -r '.color')
expect_true E-18-03 "the folder colour survives" "[[ '$colour' == '#e0af68' ]]"
expect_true E-18-03 "the favorite survives" "[[ \$(field favoriteCount) -ge 1 ]]"
ctl unpin "$ROOT_DIR/dest"; ctl clearFolderColor "$ROOT_DIR/deep"; sleep 2

expect_out E-18-04 "state lives under the state directory" "test -f ~/.local/state/omarchy/fileblade/state.json && echo yes || echo no" yes
expect_out E-18-04 "and never in the plugin checkout" "ls ~/.config/omarchy/plugins/$PLUGIN/state.json 2>/dev/null | wc -l" 0

summary
