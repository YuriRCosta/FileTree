#!/bin/bash
# Live check of keyboard-versus-pointer focus arbitration under Hyprland.
# Usage: OVM=<headless VM harness> tests/vm/window-click-focus.sh
# Assumes two tiled foot terminals named FIRST_TITLE and SECOND_TITLE are open and
# the shipped FileTree bindings are loaded in the VM.
set -u
OVM=${OVM:?set OVM to the ovm harness path}
FIRST_TITLE=${FIRST_TITLE:-focus-target}
SECOND_TITLE=${SECOND_TITLE:-second-target}
FIRST_X=${FIRST_X:-700}
SECOND_X=${SECOND_X:-1500}
WINDOW_Y=${WINDOW_Y:-600}
FIRST_SINK=/tmp/filetree-focus-first.log
SECOND_SINK=/tmp/filetree-focus-second.log
fails=0

status() { "$OVM" ipc yuricosta.filetree status 2>/dev/null; }
field() { status | jq -r ".$1"; }
active_title() { "$OVM" hypr activewindow | jq -r '.initialTitle // .title // ""'; }
sink_size() { "$OVM" ssh "stat -c %s $1" 2>/dev/null || printf missing; }
expect_value() {
  local name=$1 want=$2 got=$3
  if [[ $got == "$want" ]]; then echo "ok   $name: $got"; else echo "FAIL $name: $got wanted $want"; fails=$((fails + 1)); fi
}
close_blades() {
  "$OVM" ipc yuricosta.filetree.control closeBlade left >/dev/null
  "$OVM" ipc yuricosta.filetree.control closeBlade right >/dev/null
  sleep 0.8
}
arm_sink() {
  local x=$1 path=$2
  "$OVM" mouse click "$x" "$WINDOW_Y"; sleep 0.4
  "$OVM" key ctrl-c; sleep 0.2
  "$OVM" type "stty -icanon; cat > $path"; "$OVM" key ret; sleep 0.5
  expect_value "sink-$x-armed" 0 "$(sink_size "$path")"
}

close_blades
echo "== Super+B: a resting pointer cannot steal keyboard focus"
arm_sink "$FIRST_X" "$FIRST_SINK"
expect_value left-before "$FIRST_TITLE" "$(active_title)"
"$OVM" key meta_l-b; sleep 1
expect_value left-focused left "$(field focusedBlade)"
"$OVM" key j; sleep 0.3
expect_value left-key-captured 0 "$(sink_size "$FIRST_SINK")"
sleep 0.8; "$OVM" key k; sleep 0.3
expect_value left-rest-retained 0 "$(sink_size "$FIRST_SINK")"

echo "== real pointer motion yields focus to the window under it"
"$OVM" mouse move $((FIRST_X + 20)) "$WINDOW_Y"; sleep 0.8
expect_value motion-window "$FIRST_TITLE" "$(active_title)"
expect_value motion-released "" "$(field focusedBlade)"
"$OVM" key l; sleep 0.3
expect_value motion-key-delivered 1 "$(sink_size "$FIRST_SINK")"

close_blades
echo "== Super+Shift+B: an outside click yields right-blade focus"
arm_sink "$SECOND_X" "$SECOND_SINK"
expect_value right-before "$SECOND_TITLE" "$(active_title)"
"$OVM" key shift-meta_l-b; sleep 1
expect_value right-focused right "$(field focusedBlade)"
"$OVM" key j; sleep 0.3
expect_value right-key-captured 0 "$(sink_size "$SECOND_SINK")"
"$OVM" mouse click "$SECOND_X" "$WINDOW_Y"; sleep 0.8
expect_value right-click-window "$SECOND_TITLE" "$(active_title)"
expect_value right-click-released "" "$(field focusedBlade)"
"$OVM" key l; sleep 0.3
expect_value right-click-key-delivered 1 "$(sink_size "$SECOND_SINK")"

close_blades
echo "== keyboard focus can switch blades while the pointer remains still"
"$OVM" mouse click "$FIRST_X" "$WINDOW_Y"; sleep 0.4
"$OVM" key meta_l-b; sleep 1
expect_value switch-left left "$(field focusedBlade)"
"$OVM" key shift-meta_l-b; sleep 1
expect_value switch-right right "$(field focusedBlade)"
"$OVM" key h; sleep 0.3
expect_value switch-first-sink-unchanged 1 "$(sink_size "$FIRST_SINK")"
expect_value switch-second-sink-unchanged 1 "$(sink_size "$SECOND_SINK")"

echo "== direct focus shortcuts get the same keyboard priority"
"$OVM" ipc yuricosta.filetree.control focusBlade left >/dev/null; sleep 0.8
expect_value direct-left left "$(field focusedBlade)"
"$OVM" key j; sleep 0.3
expect_value direct-first-sink-unchanged 1 "$(sink_size "$FIRST_SINK")"
expect_value direct-second-sink-unchanged 1 "$(sink_size "$SECOND_SINK")"

"$OVM" shot window-click-focus
echo "failures: $fails"
exit $((fails > 0))
