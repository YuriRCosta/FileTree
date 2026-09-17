#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree
ctl clearClipboard; sleep 1

click_row alpha.txt
"$OVM" key y; sleep 2
expect E-13-01 "y puts the selection on the clipboard" clipboardCount 1
ctl clearClipboard; sleep 1
click_row alpha.txt
"$OVM" key ctrl-c; sleep 2
expect E-13-01 "Ctrl+C does the same" clipboardCount 1

click_row dest
"$OVM" key p; sleep 4
expect_out E-13-02 "p pastes into the selected directory" "test -f $ROOT_DIR/dest/alpha.txt && echo yes || echo no" yes

guest "printf 'movable\n' > $ROOT_DIR/movable.txt"; sleep 3
click_row movable.txt
"$OVM" key x; sleep 2
click_row dest
"$OVM" key ctrl-v; sleep 4
expect_out E-13-03 "cut and paste moves the file" "test -f $ROOT_DIR/dest/movable.txt && echo yes || echo no" yes
expect_out E-13-03 "and the source is gone" "test -e $ROOT_DIR/movable.txt && echo yes || echo no" no

click_row alpha.txt
"$OVM" key ctrl-c; sleep 2
click_row dest
"$OVM" key ctrl-v; sleep 4
expect_out E-13-04 "a second paste makes a copy rather than overwriting" "ls $ROOT_DIR/dest | grep -c 'alpha'" 2

guest "printf 'dragme\n' > $ROOT_DIR/dragme.txt"; sleep 3
src=$(row_index dragme.txt); dst=$(row_index dest)
drag_demo=$(cat <<TOML
[demo]
name = "expect-drag"
output = "Virtual-1"
capture = "screencopy"
screen = [1920, 1080]
region = { x = 0, y = 0, w = 1920, h = 1080 }
portal = false
leading_blank_ms = 200
show_keys = false
fps = 30
settle_ms = 300
gif_fps = 10
gif_width = 900

[cursor]
size = 28
fill = "#c0caf5"
outline = "#0e0e14"

[caption]
size = 30
fill = "#c0caf5"
background = "#13141c"
font = "JetBrains Mono Nerd Font"

[points]
source = [$ROW_X, $(row_y "$src")]
target = [$ROW_X, $(row_y "$dst")]

[[step]]
kind = "move"
to = "source"
ms = 300

[[step]]
kind = "hold"
ms = 400

[[step]]
kind = "drag"
hold_ms = 300
to = "target"
ease = "linear"
ms = 1800

[[step]]
kind = "hold"
ms = 900
TOML
)
guest "printf '%s\n' $(printf '%q' "$drag_demo") > /tmp/fb-drag.toml"
guest 'democtl record /tmp/fb-drag.toml --out /tmp --force' >/dev/null
sleep 3
expect_out E-13-05 "a drag onto a directory moves the file" "test -f $ROOT_DIR/dest/dragme.txt && echo yes || echo no" yes
expect_out E-13-05 "and the source is gone" "test -e $ROOT_DIR/dragme.txt && echo yes || echo no" no

pending E-13-06 "a drag paused over a collapsed folder opens it" "democtl's drag step releases the button on arrival, so it cannot hold the hover the spring timer needs"

pending E-13-09 "the cursor is a pointing finger on hover and click" "the guest screenshot never contains the cursor shape"
pending E-13-10 "the cursor is a grab hand for the whole drag" "same reason as E-13-09"
pending E-13-11 "both blades behave the same unless a plugin overrides it" "same reason as E-13-09"

click_row dest
"$OVM" key ctrl-c; sleep 2
"$OVM" key ctrl-v; sleep 4
expect_out E-13-07 "a directory is never pasted into itself" "test -e $ROOT_DIR/dest/dest && echo yes || echo no" no

ctl clearClipboard; sleep 1
ctl select "$ROOT_DIR/alpha.txt"; sleep 2
ctl copyPaths; sleep 3
wait_for "[[ -n \$(field operationNotice) ]]" 12
expect_contains E-13-08 "Copy path reports the path was copied" "$(field operationNotice)" "Copied"

summary
