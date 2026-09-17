#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR/deep"

expect E-06-01 "canGoUp is true below the root" canGoUp true
ctl up; sleep 2.5
expect E-06-01 "up moves to the parent" rootPath "$ROOT_DIR"

ctl back; sleep 2.5
expect E-06-02 "back returns to the previous root" rootPath "$ROOT_DIR/deep"
ctl forward; sleep 2.5
expect E-06-02 "forward returns again" rootPath "$ROOT_DIR"

ctl home; sleep 2.5
expect E-06-03 "home moves to the home directory" rootPath "/home/omarchy"

goto_root "$ROOT_DIR/deep"
before=$(field rootPath)
"$OVM" mouse move 80 115; sleep 1; "$OVM" mouse click 80 115; sleep 3
expect_true E-06-04 "the nav bar Up button matches the key" "[[ \$(field rootPath) != '$before' ]]"
"$OVM" mouse click 21 115; sleep 3
expect_true E-06-04 "the nav bar Back button matches the key" "[[ \$(field rootPath) == '$before' ]]"

goto_root "$ROOT_DIR"
ctl setRoot "$ROOT_DIR/does-not-exist"; sleep 3
expect E-06-05 "a missing path leaves the root alone" rootPath "$ROOT_DIR"
err=$(field locationValidationError)
expect_true E-06-05 "and reports the problem" "[[ -n '$err' && '$err' != null ]]"
ctl clearLocationError >/dev/null 2>&1; sleep 1

guest "mkdir -p '$ROOT_DIR/trailing '"
ctl_path setRoot "$ROOT_DIR/trailing "; sleep 3
expect E-06-06 "a trailing space resolves to that exact name" rootPath "$ROOT_DIR/trailing "
goto_root "$ROOT_DIR"


goto_root "$ROOT_DIR/deep"
ctl clearSearch; sleep 1
focus_tree
"$OVM" key slash; sleep 2
"$OVM" type "ab"; sleep 1
wait_for "[[ \$(field searchQuery) == ab ]]" 10
"$OVM" key backspace; sleep 2
expect E-06-07 "backspace in the search field deletes a character" searchQuery "a"
expect E-06-07 "and the folder stays where it was" rootPath "$ROOT_DIR/deep"
"$OVM" key esc; sleep 1
ctl clearSearch; sleep 1
goto_root "$ROOT_DIR"

ctl navigate "drives:///"; sleep 4
expect E-06-08 "the drives location opens" drivesMode true
expect_true E-06-08 "and lists at least the system disk" "[[ \$(field drivesCount) -ge 1 ]]"
expect E-06-08 "and up is unavailable there" canGoUp false
goto_root "$ROOT_DIR"

summary
