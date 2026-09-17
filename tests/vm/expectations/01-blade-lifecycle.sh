#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"

ensure_left_open
expect_true E-01-03 "precondition: the blade is open" "left_open"
[[ $(field focusedBlade) == left ]] || { ctl focusBlade left; wait_for "[[ \$(field focusedBlade) == left ]]" 8; }
ctl toggleBladeFocus left
wait_for "[[ \$(blade_layer left) == 0 ]]" 12
expect_true E-01-03 "super+b on a focused blade closes it" "[[ \$(blade_layer left) == 0 ]]"
expect E-01-03 "and it no longer holds focus" focusedBlade ""

ctl toggleBladeFocus left
wait_for "[[ \$(field focusedBlade) == left ]]" 12
expect E-01-01 "super+b opens the left blade" open true
expect E-01-01 "and focuses it" focusedBlade left

modules=$(field bladeModules)
expect_contains E-01-02 "left blade carries the file tree" "$modules" "files"
expect_contains E-01-02 "left blade carries properties" "$modules" "properties"

"$OVM" ssh 'pkill -x foot' >/dev/null 2>&1; sleep 1
"$OVM" ssh 'setsid foot >/dev/null 2>&1 < /dev/null &' >/dev/null 2>&1
wait_for "[[ \$("$OVM" hypr clients 2>/dev/null | jq length) -ge 1 ]]" 20
"$OVM" mouse move 900 500; sleep 1; "$OVM" mouse click 900 500
wait_for "[[ -z \$(field focusedBlade) ]]" 10
expect E-01-08 "clicking a window releases blade focus" focusedBlade ""

ensure_left_open
expect E-01-04 "precondition: the blade is open and unfocused" focusedBlade ""
ctl toggleBladeFocus left
wait_for "[[ \$(blade_layer left) == 0 ]]" 12
expect_true E-01-04 "super+b on an unfocused open blade hides it" "[[ \$(blade_layer left) == 0 ]]"
expect E-01-04 "and it stays unfocused" focusedBlade ""
ctl toggleBladeFocus left
wait_for "[[ \$(field focusedBlade) == left ]]" 12
expect E-01-04 "the next press opens and focuses it again" focusedBlade left

"$OVM" ssh 'pkill -x foot' >/dev/null 2>&1; sleep 1
"$OVM" ssh 'setsid foot -e sh -c "stty -icanon -echo; cat > /tmp/keys.log" >/dev/null 2>&1 < /dev/null &' >/dev/null 2>&1
wait_for "[[ \$("$OVM" hypr clients 2>/dev/null | jq length) -ge 1 ]]" 20
"$OVM" mouse move 900 500; sleep 1; "$OVM" mouse click 900 500
wait_for "[[ -z \$(field focusedBlade) ]]" 10
guest ': > /tmp/keys.log'
"$OVM" mouse move 700 1073; sleep 2
ensure_left_open
ctl focusBlade left
wait_for "[[ \$(field focusedBlade) == left ]]" 12
active=$("$OVM" hypr activewindow 2>/dev/null | jq -r '.class // "none"')
before=$(field selectedPath)
"$OVM" key j; sleep 1; "$OVM" key j; sleep 2
after=$(field selectedPath)
sink=$(guest 'cat /tmp/keys.log' | tr -d '\r\n')
expect_true E-01-06 "keys reach the tree while a window is active" "[[ '$before' != '$after' ]]"
expect_true E-01-06 "and the terminal receives nothing" "[[ -z '$sink' ]]"
expect_true E-01-07 "activewindow still names the window, which is correct" "[[ '$active' == foot ]]"

"$OVM" mouse move "$ROW_X" "$(row_y 2)"; sleep 2
ctl focusBlade left; wait_for "[[ \$(field focusedBlade) == left ]]" 10
"$OVM" ssh 'pkill -x foot' >/dev/null 2>&1
wait_for "[[ \$("$OVM" hypr clients 2>/dev/null | jq length) == 0 ]]" 20
wait_for "[[ \$(field focusedBlade) == left || \$(field focusedBlade) == right ]]" 15
expect_true E-01-09 "closing the last window focuses a blade" "[[ \$(field focusedBlade) == left || \$(field focusedBlade) == right ]]"

ensure_left_open
ctl closeBlade right
wait_for "[[ \$(blade_layer right) == 0 ]]" 12
ctl toggleBladeFocus right
wait_for "[[ \$(field focusedBlade) == right ]]" 12
expect E-01-05 "super+shift+b focuses the right blade" focusedBlade right
expect_true E-01-05 "and the left blade stays open" "left_open"

ctl focusDirection l
wait_for "[[ \$(field focusedBlade) == left ]]" 12
expect E-01-10 "focusDirection l returns to the left blade" focusedBlade left
ctl focusDirection r
wait_for "[[ \$(field focusedBlade) == right ]]" 12
expect E-01-10 "focusDirection r reaches the right blade" focusedBlade right
ctl focusDirection l
wait_for "[[ \$(field focusedBlade) == left ]]" 12

goto_root "$ROOT_DIR/deep"
ensure_left_open
ctl focusBlade left; sleep 1
before_root=$(field rootPath)
"$OVM" mouse move 80 115; sleep 1
"$OVM" mouse click 80 115; sleep 3
expect_true E-01-11 "the nav bar Up button works on the first click" "[[ '$before_root' != \$(field rootPath) ]]"


"$OVM" ssh 'pkill -x foot' >/dev/null 2>&1; sleep 1
"$OVM" ssh 'setsid foot >/dev/null 2>&1 < /dev/null &' >/dev/null 2>&1
wait_for "[[ \$("$OVM" hypr clients 2>/dev/null | jq length) -ge 1 ]]" 20
ensure_left_open
ctl focusBlade left; wait_for "[[ \$(field focusedBlade) == left ]]" 10
ctl toggleBladeFocus left
wait_for "[[ \$(blade_layer left) == 0 ]]" 12
active_class() { "$OVM" hypr activewindow 2>/dev/null | jq -r '.class // ""'; }
wait_for "[[ -n \$(active_class) && \$(active_class) != org.quickshell ]]" 10
expect_true E-01-12 "closing the focused blade hands focus to an application window" "[[ -n \$(active_class) && \$(active_class) != org.quickshell ]]"
expect E-01-12 "and no blade holds focus" focusedBlade ""
"$OVM" ssh 'pkill -x foot' >/dev/null 2>&1
ensure_left_open

ensure_left_open
ctl focusProperties >/dev/null; sleep 2
"$OVM" key esc
wait_for "[[ \$(blade_layer left) == 0 ]]" 12
expect_true E-01-13 "escape in the properties section closes the blade" "[[ \$(blade_layer left) == 0 ]]"
ensure_left_open

summary
