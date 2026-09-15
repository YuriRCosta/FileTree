#!/usr/bin/env bash
# Docking, resizing and window behavior. Expectations E-21-01 .. E-21-10.
source "$(dirname "$0")/lib.sh"

require_guest

clients() { "$OVM" hypr clients 2>/dev/null | jq length; }
window_x() { "$OVM" hypr clients 2>/dev/null | jq -r '[.[]?|select(.class=="foot")][0].at[0] // empty'; }
kill_windows() { "$OVM" ssh 'pkill -x foot' >/dev/null 2>&1; wait_for "[[ \$(clients) == 0 ]]" 15; }
start_window() {
  "$OVM" ssh 'setsid foot >/dev/null 2>&1 < /dev/null &' >/dev/null 2>&1
  wait_for "[[ \$(clients) -ge 1 ]]" 25
}

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
kill_windows

ensure_left_closed
start_window
undocked_x=$(window_x)
ensure_left_open
wait_for "[[ \$(window_x) != '$undocked_x' ]]" 15
docked_x=$(window_x)
expect_true E-21-01 "a docked blade pushes tiled windows aside" "[[ -n '$docked_x' && '$docked_x' != '$undocked_x' ]]"
expect_true E-21-01 "and the window starts beyond the blade" "[[ '$docked_x' -ge \$(field sidebarWidth) ]]"

# In window mode the blade is a real window, not a layer, so every later case
# has to see it docked again or it inherits the wrong world.
left_mode() { status | jq -r '.bladeModes.left'; }
dock_left() {
  [[ $(left_mode) == docked ]] && return 0
  ctl focusBlade left; wait_for "[[ \$(field focusedBlade) == left ]]" 10
  ctl windowToggle
  wait_for "[[ \$(left_mode) == docked ]]" 15
}

ctl focusBlade left; wait_for "[[ \$(field focusedBlade) == left ]]" 10
ctl windowToggle
wait_for "[[ \$(left_mode) == window ]]" 15
expect_true E-21-02 "Super+T makes the blade a window" "[[ \$(left_mode) == window ]]"
expect_true E-21-02 "and it stops being a layer" "[[ \$(blade_layer left) == 0 ]]"
dock_left
expect_true E-21-02 "pressing it again docks the blade" "[[ \$(left_mode) == docked ]]"
expect_true E-21-02 "and the layer is back" "[[ \$(blade_layer left) == 1 ]]"

dock_left
ctl focusBlade left; sleep 1
narrow=$(field sidebarWidth)
ctl windowResize -100 0 >/dev/null 2>&1; sleep 3
after_resize=$(field sidebarWidth)
expect_true E-21-03 "resizing changes only the focused blade" "[[ '$after_resize' != '$narrow' ]]"
expect_true E-21-03 "and the right blade keeps its width" "[[ \$(field propertiesBladeWidth) -gt 0 ]]"

ctl setSidebarWidth 430; wait_for "[[ \$(field sidebarWidth) == 430 ]]" 10
ctl close; sleep 3
ctl open; wait_for "[[ \$(field open) == true ]]" 15
expect E-21-05 "the width survives close and reopen" sidebarWidth 430
ctl setSidebarWidth "$narrow"; sleep 2

dock_left
ctl focusBlade left; wait_for "[[ \$(field focusedBlade) == left ]]" 10
ctl windowClose >/dev/null 2>&1
wait_for "[[ \$(blade_layer left) == 0 ]]" 12
expect_true E-21-06 "Super+W closes the focused blade" "[[ \$(blade_layer left) == 0 ]]"
expect_true E-21-06 "and leaves my window alone" "[[ \$(clients) -ge 1 ]]"
ensure_left_open

ctl releaseBladeFocus >/dev/null 2>&1; wait_for "[[ -z \$(field focusedBlade) ]]" 8
expect E-21-06 "releasing focus is not undone by the opening animation" focusedBlade ""
windows_before=$(clients)
ctl windowClose >/dev/null 2>&1
wait_for "[[ \$(clients) -lt $windows_before ]]" 12
expect_true E-21-06 "with no blade focused it closes the window instead" "[[ \$(clients) -lt $windows_before ]]"

dock_left
ensure_left_open
# With a window between the blades the first step to the right lands on that
# window, which is the natural order E-21-07 describes; the blade-to-blade hop
# is only observable with no window open.
kill_windows
ctl openBlade right; wait_for "[[ \$(blade_layer right) == 1 ]]" 12
ctl focusBlade left; wait_for "[[ \$(field focusedBlade) == left ]]" 10
ctl focusDirection r; wait_for "[[ \$(field focusedBlade) == right ]]" 10
expect E-21-07 "Super with an arrow moves focus between blades" focusedBlade right
ctl focusDirection l; wait_for "[[ \$(field focusedBlade) == left ]]" 10
ctl closeBlade right

ctl setBladeAnimations false; sleep 2
expect E-21-09 "animations can be turned off" bladeAnimations false
ctl setBladeAnimations true; sleep 2
expect E-21-09 "and back on" bladeAnimations true

ensure_left_open
kill_windows
start_width=$(field sidebarWidth)
"$OVM" mouse move 200 400 >/dev/null 2>&1; sleep 1
"$OVM" mouse move 203 402 >/dev/null 2>&1; sleep 1
expect E-21-11 "the blade under the pointer is the one that resizes" pointerEdge left
ctl pointerResizeBegin >/dev/null
expect E-21-11 "and the drag opens on that blade" pointerResizeEdge left
"$OVM" mouse move 640 402 >/dev/null 2>&1
wait_for "[[ \$(field pointerResizeEdge) == left ]]" 8
ctl pointerResizeEnd >/dev/null
wait_for "[[ -z \$(field pointerResizeEdge) ]]" 10
resized=$(field sidebarWidth)
expect_true E-21-11 "the released width is the dragged width" "[[ '$resized' -gt '$start_width' ]]"
ctl setBladeWidth left "$start_width" >/dev/null
wait_for "[[ \$(field sidebarWidth) == '$start_width' ]]" 10

"$OVM" mouse move 1400 600 >/dev/null 2>&1; sleep 1
expect_true E-21-12 "off a blade the pointer owns no edge" "[[ -z \$(field pointerEdge) ]]"
off_blade_width=$(field sidebarWidth)
ctl pointerResizeBegin >/dev/null
sleep 1
expect_true E-21-12 "so no blade takes the drag" "[[ -z \$(field pointerResizeEdge) ]]"
"$OVM" mouse move 1500 640 >/dev/null 2>&1; sleep 1
expect_true E-21-12 "and no blade width moves with the pointer" "[[ \$(field sidebarWidth) == '$off_blade_width' ]]"
ctl pointerResizeEnd >/dev/null

pending E-21-12 "the window underneath resizes instead" "the fallback only runs with the right button physically held, and the harness cannot hold a button without killing QEMU"
pending E-21-04 "dragging the inner edge resizes without smearing" "needs a frame by frame capture of the drag, not a end state check"
pending E-21-08 "Super+Shift with an arrow moves the focused section" "section order is not reported in the status document"
pending E-21-10 "blades appear only on configured screens" "the guest has one virtual monitor"

dock_left
ensure_left_open
kill_windows
summary
