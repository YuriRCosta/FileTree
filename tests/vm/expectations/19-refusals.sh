#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree

click_row alpha.txt
"$OVM" key r; sleep 2.5; "$OVM" type "data.json"; sleep 1.2; "$OVM" key ret; sleep 3
err=$(field operationError)
expect_true E-19-01 "a refusal reaches the person" "[[ -n '$err' && '$err' != null ]]"
expect_contains E-19-01 "and says what went wrong" "$err" "already exists"

click_row alpha.txt
"$OVM" key a; sleep 2.5; "$OVM" type "after-error.txt"; sleep 1.2; "$OVM" key ret; sleep 3
expect_out E-19-02 "the next action still works" "test -f $ROOT_DIR/after-error.txt && echo yes || echo no" yes

guest "mkdir -p $ROOT_DIR/locked && chmod 000 $ROOT_DIR/locked"; sleep 2
ctl expandPath "$ROOT_DIR/locked"; sleep 4
locked_error=$("$OVM" ipc "$PLUGIN" tree 300 2>/dev/null | jq -r '[.entries[]?|select(.name=="locked")][0].error')
expect_true E-19-03 "an unreadable directory reports why" "[[ -n '$locked_error' && '$locked_error' != null && '$locked_error' != '' ]]"
guest "chmod 755 $ROOT_DIR/locked"

guest "printf 'gone\n' > $ROOT_DIR/vanish.txt"; sleep 3
ctl select "$ROOT_DIR/vanish.txt"; sleep 2
ctl copySelection false; sleep 2
guest "rm -f $ROOT_DIR/vanish.txt"; sleep 2
ctl paste "$ROOT_DIR/dest"; sleep 4
wait_for "[[ \$(field operationBusy) == false ]]" 20
gone_error=$(field operationError)
expect_true E-19-04 "a vanished file reports a problem" "[[ -n '$gone_error' && '$gone_error' != null ]]"
expect_out E-19-04 "and nothing lands at the destination" "test -e $ROOT_DIR/dest/vanish.txt && echo yes || echo no" no

long_name=$(printf 'x%.0s' $(seq 1 300))
click_row alpha.txt
"$OVM" key a; sleep 2.5; "$OVM" type "$long_name"; sleep 1.5; "$OVM" key ret; sleep 3
expect_out E-19-05 "an over-long name creates nothing" "ls $ROOT_DIR | grep -c 'xxxxxxxxxx' || true" 0

guest "mkdir -p $ROOT_DIR/wide && cd $ROOT_DIR/wide && seq 1 4000 | xargs -I{} touch f{}" >/dev/null
ctl setRoot "$ROOT_DIR/wide"; sleep 3
wait_for "[[ \$(field operationBusy) == false ]]" 40
settled=$(field operationBusy)
expect_true E-19-06 "a heavy listing settles rather than spinning forever" "[[ '$settled' == false ]]"
expect_true E-19-06 "and the rows arrive" "[[ \$(field treeEntries) -ge 1 ]]"
goto_root "$ROOT_DIR"

summary
