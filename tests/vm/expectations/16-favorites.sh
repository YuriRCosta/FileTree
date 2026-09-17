#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

favs() { "$OVM" ipc "$PLUGIN" favorites 2>/dev/null | jq -r '[.[]?|.path]|join(",")'; }

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
for p in $(favs | tr ',' ' '); do [[ -n $p ]] && ctl unpin "$p"; done; sleep 2

ctl toggleFavorite "$ROOT_DIR/deep"; sleep 3
expect_contains E-16-01 "pinning adds the directory" "$(favs)" "$ROOT_DIR/deep"
expect_true E-16-01 "and the count rises" "[[ \$(field favoriteCount) -ge 1 ]]"

ctl toggleFavorite "$ROOT_DIR/deep"; sleep 3
expect_missing E-16-02 "unpinning removes it" "$(favs)" "$ROOT_DIR/deep"

ctl toggleFavorite "$ROOT_DIR/dest"; sleep 3
restart_shell
open_left; goto_root "$ROOT_DIR"
expect_contains E-16-03 "favorites survive a shell restart" "$(favs)" "$ROOT_DIR/dest"

guest "mkdir -p $ROOT_DIR/vanishing"; sleep 2
ctl toggleFavorite "$ROOT_DIR/vanishing"; sleep 3
guest "rmdir $ROOT_DIR/vanishing"; sleep 3
ctl refresh; sleep 3
expect_true E-16-04 "a missing favorite does not break the listing" "[[ \$(field treeEntries) -ge 1 ]]"
ctl unpin "$ROOT_DIR/vanishing"; ctl unpin "$ROOT_DIR/dest"; sleep 2

summary
