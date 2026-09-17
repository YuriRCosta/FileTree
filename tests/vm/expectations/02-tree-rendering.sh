#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
ctl setShowHidden true; sleep 1
goto_root "$ROOT_DIR"

on_disk=$(guest "ls -A $ROOT_DIR | wc -l")
listed=$("$OVM" ipc "$PLUGIN" tree 200 2>/dev/null | jq '[.entries[]?|select(.depth==1)]|length')
expect_true E-02-01 "every visible child is listed once" "[[ '$listed' == '$on_disk' ]]"

order=$("$OVM" ipc "$PLUGIN" tree 200 2>/dev/null | jq -r '[.entries[]?|select(.depth==1)|(if .isDir then "d" else "f" end)]|join("")')
expect_true E-02-02 "directories sort before files" "[[ ! '$order' =~ f+d ]]"

empty_row=$("$OVM" ipc "$PLUGIN" tree 200 2>/dev/null | jq -r '[.entries[]?|select(.name=="empty")][0]|tostring')
expect_missing E-02-03 "an empty directory is never labelled empty" "$empty_row" '"isEmpty"'
expect_missing E-02-03 "and emptiness is never known" "$empty_row" '"emptyKnown"'

link_kind=$("$OVM" ipc "$PLUGIN" tree 200 2>/dev/null | jq -r '[.entries[]?|select(.name=="link-alpha.txt")][0].isSymlink')
expect_true E-02-04 "a symlink row is marked as one" "[[ '$link_kind' == true ]]"

guest "printf 'new\n' > $ROOT_DIR/zz-watch.txt"; sleep 4
expect_contains E-02-05 "a new file appears without a refresh" "$(tree_names)" "zz-watch.txt"

guest "rm -f $ROOT_DIR/zz-watch.txt"; sleep 4
expect_missing E-02-06 "a removed file disappears without a refresh" "$(tree_names)" "zz-watch.txt"

guest "printf 'inside\n' > $ROOT_DIR/empty/late.txt"; sleep 3
ctl refresh; sleep 3
ctl expandPath "$ROOT_DIR/empty"; sleep 3
expect_contains E-02-07 "refresh picks up a change inside a collapsed directory" "$(tree_names)" "late.txt"

summary
