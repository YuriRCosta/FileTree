#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree

guest "rm -rf $ROOT_DIR/bundle $ROOT_DIR/bundle.tar.gz
  mkdir -p $ROOT_DIR/bundle && printf 'one\n' > $ROOT_DIR/bundle/one.txt && printf 'two\n' > $ROOT_DIR/bundle/two.txt
  cd $ROOT_DIR && tar czf bundle.tar.gz -C bundle . && rm -rf bundle"
ctl refresh; sleep 4

click_row bundle.tar.gz
"$OVM" key m; sleep 2.5
"$OVM" type "extract"; sleep 1.5
"$OVM" key ret; sleep 6
wait_for "[[ \$(field operationBusy) == false ]]" 40
expect_out E-28-01 "extract places the contents beside the archive" "ls -d $ROOT_DIR/bundle 2>/dev/null | wc -l" 1
expect_out E-28-01 "and the files are there" "test -f $ROOT_DIR/bundle/one.txt && echo yes || echo no" yes

click_row bundle.tar.gz
"$OVM" key m; sleep 2.5
"$OVM" type "extract"; sleep 1.5
"$OVM" key ret; sleep 6
wait_for "[[ \$(field operationBusy) == false ]]" 40
again_error=$(field operationError)
expect_true E-28-02 "a second extract into a populated destination is refused" "[[ -n '$again_error' && '$again_error' != null ]]"
expect_out E-28-02 "and the existing files are untouched" "cat $ROOT_DIR/bundle/one.txt" one

guest "rm -rf $ROOT_DIR/heavy $ROOT_DIR/heavy-dest
  mkdir -p $ROOT_DIR/heavy $ROOT_DIR/heavy-dest
  cd $ROOT_DIR/heavy && for ((i=1; i<=20000; i++)); do printf 'payload\n' > item\$i.txt; done"
ctl refresh; sleep 4
ctl select "$ROOT_DIR/heavy"; sleep 2
ctl copySelection false; sleep 2
ctl paste "$ROOT_DIR/heavy-dest"
saw_label=$(field operationLabel)
saw_busy=$(field operationBusy)
if [[ $saw_busy == true ]]; then
  expect_true E-28-03 "a long operation says what it is doing" '[[ -n $saw_label && $saw_label != null ]]'
  ctl cancelOperation "$(field activeOperationId)" >/dev/null 2>&1
else
  expect_out E-28-03 "a completed operation publishes all its files" "find $ROOT_DIR/heavy-dest/heavy -type f | wc -l" 20000
  pending E-28-04 "a running copy can be cancelled" "the copy finished before cancellation could be requested"
fi
wait_for "[[ \$(field operationBusy) == false ]]" 60
expect_true E-28-04 "stopping settles rather than hanging" "[[ \$(field operationBusy) == false ]]"
expect_out E-28-04 "and the destination is readable afterwards" "test -r $ROOT_DIR/heavy-dest && echo yes" yes

ctl select "$ROOT_DIR/alpha.txt"
ctl copySelection false
ctl paste "$ROOT_DIR/dest"
wait_for '[[ $(field operationBusy) == false ]]' 20
guest "printf 'changed outside FileTree\n' >> $ROOT_DIR/dest/alpha.txt"
ctl focusTree
"$OVM" key u
sleep 2
expect_true E-28-05 "undo refuses to remove an externally changed copy" '[[ -n $(field operationError) ]]'
"$OVM" key shift-u
sleep 2
expect_true E-28-05 "Shift+U clears the refused undo" '[[ -z $(field operationError) ]]'
expect_out E-28-05 "and leaves the changed copy intact" "tail -1 $ROOT_DIR/dest/alpha.txt" "changed outside FileTree"

guest "rm -rf $ROOT_DIR/heavy $ROOT_DIR/heavy-dest $ROOT_DIR/bundle $ROOT_DIR/bundle.tar.gz"
summary
