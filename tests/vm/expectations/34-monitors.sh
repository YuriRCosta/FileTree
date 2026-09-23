#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest
REPO=$(cd "$(dirname "$0")/../../.." && pwd)
EVIDENCE=$REPO/.claude/monitor-acceptance
mkdir -p "$EVIDENCE"
secondary=FILETREE-TEST
primary=$("$OVM" hypr monitors | jq -r '.[0].name')
case_dir=""
created_output=0
sink_pids=""
layout_path=$(field bladeLayoutPath)
original_root=$(field rootPath)

reply() {
  local command="omarchy-shell $PLUGIN.control" argument
  for argument in "$@"; do command+=" $(printf '%q' "$argument")"; done
  guest "$command" | tr -d '\r'
}

blades() { "$OVM" ipc "$PLUGIN" blades; }
owner() { status | jq -r --arg edge "$1" '.bladeScreens[$edge]'; }
focused_monitor() { "$OVM" hypr monitors | jq -r '.[] | select(.focused) | .name'; }
layers_on() {
  "$OVM" hypr layers | jq --arg screen "$1" --arg ns "omarchy-filetree-$2" \
    '[.[$screen] | .. | objects | select(.namespace? == $ns)] | length'
}
layer_shape() {
  "$OVM" hypr layers | jq -c --arg a "$primary" --arg b "$secondary" \
    '[[$a,$b][] as $screen | ["left","right"][] as $edge |
      [.[$screen] | .. | objects | select(.namespace? == ("omarchy-filetree-" + $edge))] | length]'
}
zones() {
  "$OVM" hypr monitors | jq -c --arg a "$primary" --arg b "$secondary" \
    '[[$a,$b][] as $screen | (.[] | select(.name == $screen)) | .reserved[0], .reserved[2]]'
}
layout_content() { blades | jq -Sc '.blades | map_values({width, mode, slots})'; }
capture() {
  local name=$1 screen
  status > "$EVIDENCE/$name-status.json"
  blades > "$EVIDENCE/$name-blades.json"
  "$OVM" hypr monitors > "$EVIDENCE/$name-monitors.json"
  "$OVM" hypr layers > "$EVIDENCE/$name-layers.json"
  "$OVM" hypr activewindow > "$EVIDENCE/$name-window.json"
  while IFS= read -r screen; do
    guest "grim -o $(printf '%q' "$screen") $case_dir/shot.png && base64 -w0 $case_dir/shot.png" \
      | base64 -d > "$EVIDENCE/$name-$screen.png"
  done < <("$OVM" hypr monitors | jq -r '.[].name')
}
equal() {
  [[ $3 == "$4" ]] && pass "$1" "$2" || fail "$1" "$2" "got [$3], wanted [$4]"
}
await_shape() {
  wait_for "[[ \$(layer_shape) == '$3' ]]" 12
  equal "$1" "$2" "$(layer_shape)" "$3"
}
abort() { fail harness "$1" "$2"; summary; }
close_edges() {
  ctl hideDropWheel
  ctl closeBlade left
  ctl closeBlade right
  wait_for "[[ \$(layer_shape) == '[0,0,0,0]' ]]" 12
}
focus_output() {
  guest "democtl move --output $(printf '%q' "$1") 1000 500" >/dev/null
  sleep 0.8
  guest "democtl click --output $(printf '%q' "$1") 1000 500" >/dev/null
  wait_for "[[ \$(focused_monitor) == '$1' ]]" 8 || abort "focus output" "$1 never became focused"
}
create_output() {
  guest "hyprctl output create headless $secondary" >/dev/null
  created_output=1
  wait_for "\"\$OVM\" hypr monitors | jq -e --arg name '$secondary' 'any(.[]; .name == \$name)'" 8 \
    || abort "create output" "$secondary absent"
  guest "hyprctl eval 'hl.monitor({output=\"$secondary\", mode=\"1920x1080@60\", position=\"2160x-80\", scale=1.25})'" >/dev/null
  wait_for "\"\$OVM\" hypr monitors | jq -e --arg name '$secondary' 'any(.[]; .name == \$name and .scale == 1.25 and .x == 2160 and .y == -80)'" 8 \
    || abort "configure output" "fractional scale or geometry not applied"
}
remove_output() {
  guest "hyprctl output remove $secondary" >/dev/null
  wait_for "\"\$OVM\" hypr monitors | jq -e --arg name '$secondary' 'all(.[]; .name != \$name)'" 8 \
    || abort "remove output" "$secondary still present"
  created_output=0
}
start_sink() {
  local output=$1 label=$2 pid
  focus_output "$output"
  guest "setsid foot --title=FileTreeMonitor-$label sh -c 'stty -icanon -echo; cat > \"\$1\"' sh $case_dir/$label.keys >/dev/null 2>&1 </dev/null &" >/dev/null
  wait_for "\"\$OVM\" hypr clients | jq -e 'any(.[]; .initialTitle == \"FileTreeMonitor-$label\")'" 15 \
    || abort "start keyboard sink" "$label absent"
  pid=$("$OVM" hypr clients | jq -r --arg title "FileTreeMonitor-$label" '.[] | select(.initialTitle == $title) | .pid')
  sink_pids+=" $pid"
  focus_output "$output"
}
input_reaches() {
  local id=$1 output=$2 label=$3 before after
  focus_output "$output"
  before=$(guest "stat -c %s $case_dir/$label.keys")
  "$OVM" key x
  sleep 0.6
  after=$(guest "stat -c %s $case_dir/$label.keys")
  equal "$id" "typed input reaches the window on $output" "$after" "$((before + 1))"
  equal "$id" "the terminal is compositor-active on $output" \
    "$("$OVM" hypr activewindow | jq -r .initialTitle)" "FileTreeMonitor-$label"
}
cleanup() {
  local code=$? pid
  trap - EXIT
  if [[ -n $case_dir ]]; then
    close_edges >/dev/null 2>&1
    for pid in $sink_pids; do
      [[ $pid =~ ^[0-9]+$ ]] && guest "kill $pid" >/dev/null 2>&1
    done
    ((created_output)) && guest "hyprctl output remove $secondary" >/dev/null 2>&1
    if guest "test -f $case_dir/bindings.lua"; then
      guest "cp $case_dir/bindings.lua ~/.config/hypr/bindings.lua; hyprctl reload" >/dev/null 2>&1
    fi
    if guest "test -f $case_dir/blades.json"; then
      "$(dirname "$0")/../stop-shell" >/dev/null 2>&1
      guest "cp $case_dir/blades.json $(printf '%q' "$layout_path")" >/dev/null 2>&1
      "$OVM" restart-shell >/dev/null 2>&1
      wait_for "[[ -n \$(field rootPath) ]]" 25
      ctl setRoot "$original_root"
    fi
    guest "rm -rf -- $case_dir" >/dev/null 2>&1
  fi
  exit "$code"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

[[ $("$OVM" hypr monitors | jq length) == 1 ]] || abort "isolated setup" "start with exactly one output"
status | jq -e 'has("bladeScreens") and has("monitorLock")' >/dev/null \
  || abort "candidate IPC" "invocation-mode status fields missing"
case_dir=$(guest 'mktemp -d /tmp/filetree-monitors.XXXXXX')
[[ $case_dir == /tmp/filetree-monitors.* ]] || abort "temporary directory" "invalid path"
guest "cp ~/.config/hypr/bindings.lua $case_dir/bindings.lua; cp $(printf '%q' "$layout_path") $case_dir/blades.json"
guest "cat $GUEST_PLUGIN/examples/filetree-bindings.lua >> ~/.config/hypr/bindings.lua; hyprctl reload" >/dev/null
equal harness "guest bindings parse cleanly" "$(guest 'hyprctl configerrors')" ""
dock_blades
close_edges
create_output
start_sink "$primary" a
start_sink "$secondary" b
baseline_zones=$(zones)
baseline_layout=$(layout_content)
ctl setMonitorMode active ""
focus_output "$primary"
"$OVM" key meta_l-b
await_shape E-34-01 "Active opens left only on its invocation output" '[1,0,0,0]'
equal E-34-01 "left records its invocation output" "$(owner left)" "$primary"
equal E-34-01 "invoked blade receives focus" "$(field focusedBlade)" left
capture active-a
input_reaches E-34-01 "$secondary" b
await_shape E-34-01 "ordinary focus movement leaves the blade on A" '[1,0,0,0]'
equal E-34-01 "only A reserves additional left space" \
  "$(jq -n --argjson before "$baseline_zones" --argjson after "$(zones)" \
    '$after[0] > $before[0] and $after[1:] == $before[1:]')" true
capture active-a-focus-b

for edge in left right; do
  if [[ $edge == left ]]; then chord=meta_l-b; a_shape='[1,0,0,0]'; b_shape='[0,0,1,0]'
  else chord=shift-meta_l-b; a_shape='[0,1,0,0]'; b_shape='[0,0,0,1]'; fi
  close_edges
  focus_output "$primary"
  "$OVM" key "$chord"
  await_shape E-34-02 "$edge opens on A" "$a_shape"
  input_reaches E-34-02 "$secondary" b
  "$OVM" key "$chord"
  await_shape E-34-02 "one press on B closes $edge on A" '[0,0,0,0]'
  equal E-34-02 "closed $edge clears its owner" "$(owner "$edge")" ""
  "$OVM" key "$chord"
  await_shape E-34-02 "next press opens $edge on B" "$b_shape"
  equal E-34-02 "$edge records B" "$(owner "$edge")" "$secondary"
  equal E-34-02 "$edge receives keyboard focus" "$(field focusedBlade)" "$edge"
  capture "active-$edge-b"
done

close_edges
focus_output "$primary"
"$OVM" key meta_l-b
focus_output "$secondary"
"$OVM" key shift-meta_l-b
await_shape E-34-03 "edges retain separate invocation outputs" '[1,0,0,1]'
equal E-34-03 "left remains anchored on A" "$(owner left)" "$primary"
equal E-34-03 "right remains anchored on B" "$(owner right)" "$secondary"
equal E-34-03 "there is still one unchanged shared layout" "$(layout_content)" "$baseline_layout"
capture independent-edges
close_edges
focus_output "$secondary"
ctl open
await_shape E-34-03 "opening all records B for both edges" '[0,0,1,1]'
ctl close
await_shape E-34-03 "closing all clears both visible edges" '[0,0,0,0]'
equal E-34-03 "closing all clears both owners" "$(status | jq -c .bladeScreens)" '{"left":"","right":""}'
focus_output "$primary"
ctl open
await_shape E-34-03 "reopening all records A without a focus change afterward" '[1,1,0,0]'
close_edges
equal E-34-04 "a detected output can be locked" "$(reply setMonitorMode locked "$secondary")" locked
focus_output "$primary"
"$OVM" key meta_l-b
await_shape E-34-04 "locked left opens on B when invoked from A" '[0,0,1,0]'
input_reaches E-34-04 "$primary" a
"$OVM" key shift-meta_l-b
await_shape E-34-04 "locked right also opens on B" '[0,0,1,1]'
equal E-34-04 "an unknown lock is rejected" "$(reply setMonitorMode locked FILETREE-MISSING)" unknown-monitor
equal E-34-04 "invalid lock preserves the chosen output" "$(field monitorLock)" "$secondary"
equal E-34-04 "unknown explicit focus does not fall back" "$(reply focusBladeOn left FILETREE-MISSING)" unknown-monitor
equal E-34-04 "ineligible explicit focus is rejected" "$(reply focusBladeOn left "$primary")" no-screen
capture locked-b
close_edges
ctl setMonitorMode all ""
ctl openBlade left
ctl openBlade right
await_shape E-34-05 "All mirrors both edges" '[1,1,1,1]'
equal E-34-05 "All reports no invocation owners" "$(status | jq -c .bladeScreens)" '{"left":"","right":""}'
capture all
close_edges
ctl setMonitorMode active ""
focus_output "$primary"
saved_width=$(field sidebarWidth)
ctl setSidebarWidth 1300
"$OVM" key meta_l-b
await_shape E-21-10a "wide blade opens on the large output" '[1,0,0,0]'
equal E-21-10a "large output preserves the chosen width" "$(field sidebarWidth)" 1300
close_edges
focus_output "$secondary"
"$OVM" key meta_l-b
await_shape E-21-10a "wide blade reopens on the smaller output" '[0,0,1,0]'
equal E-21-10a "rendering on a smaller output preserves stored width" "$(field sidebarWidth)" 1300
equal E-21-10a "reserved width respects the smaller output limit" \
  "$("$OVM" hypr monitors | jq -r --arg name "$secondary" '.[] | select(.name == $name) |
    .reserved[0] > 1000 and .reserved[0] <= ((.width / .scale * 0.72) | ceil)')" true
capture clamped-width
close_edges
focus_output "$primary"
ctl setSidebarWidth "$saved_width"
equal E-34-05 "legacy Primary maps to locked" "$(reply setMonitorMode primary "")" locked
equal E-34-05 "legacy Primary locks the first output" "$(field monitorLock)" "$primary"
focus_output "$secondary"
"$OVM" key meta_l-b
await_shape E-34-05 "Primary lock remains on A when invoked on B" '[1,0,0,0]'

close_edges
ctl setMonitorMode locked "$secondary"
focus_output "$primary"
"$OVM" key meta_l-b
await_shape E-34-06 "locked blade is visible before unplug" '[0,0,1,0]'
remove_output
await_shape E-34-06 "unplug never migrates a locked blade to A" '[0,0,0,0]'
equal E-34-06 "unplug preserves the lock" "$(field monitorLock)" "$secondary"
equal E-34-06 "missing lock refuses invocation" "$(reply toggleBladeFocus right)" no-screen
capture locked-disconnected
create_output
await_shape E-34-06 "same named output becomes eligible again" '[0,0,1,0]'
equal E-34-06 "reconnect preserves the lock" "$(field monitorLock)" "$secondary"

close_edges
ctl setMonitorMode active ""
focus_output "$secondary"
"$OVM" key meta_l-b
await_shape E-34-07 "Active records B before unplug" '[0,0,1,0]'
remove_output
await_shape E-34-07 "unplug does not migrate an Active blade" '[0,0,0,0]'
equal E-34-07 "Active retains its unavailable owner until closed" "$(owner left)" "$secondary"
focus_output "$primary"
equal E-34-07 "first press closes the unavailable edge" "$(reply toggleBladeFocus left)" closed
equal E-34-07 "closing clears the unavailable owner" "$(owner left)" ""
"$OVM" key meta_l-b
await_shape E-34-07 "reopening explicitly selects A" '[1,0,0,0]'
create_output
await_shape E-34-07 "returning B does not move the reopened blade" '[1,0,0,0]'

close_edges
ctl setMonitorMode locked "$primary"
guest "printf 'keep\n' > $case_dir/keep.txt"
ctl setRoot "$case_dir"
ctl select "$case_dir/keep.txt"
focus_output "$primary"
equal E-34-08 "wheel accepts a global point on B" "$(reply showDropWheel 2760 320)" open
wait_for "[[ \$(layers_on '$secondary' drop) == 1 ]]" 10
equal E-34-08 "wheel draws on B despite the blade lock on A" "$(layers_on "$secondary" drop)" 1
equal E-34-08 "wheel does not draw on A" "$(layers_on "$primary" drop)" 0
capture wheel-b
ctl hideDropWheel
equal E-34-08 "wheel rejects a point in the gap" "$(reply showDropWheel 2040 400)" off-screen
equal E-34-08 "wheel rejects a point outside every output" "$(reply showDropWheel -1000 -1000)" off-screen
equal E-34-08 "rejected wheel stays closed" "$(status | jq -r .dropWheel.open)" false
start_sink "$secondary" trash-b
close_edges
ctl setMonitorMode active ""
focus_output "$primary"
ctl focusBlade left
await_shape E-34-10 "trash owner is visible on A" '[1,0,0,0]'
ctl select "$case_dir/keep.txt"
sleep 1
"$OVM" key delete
wait_for "[[ \$(field pendingTrashCount) == 1 ]]" 10
equal E-34-10 "Delete opens a pending trash question" "$(field pendingTrashCount)" 1
capture trash-on-owner
input_reaches E-34-10 "$secondary" trash-b
equal E-34-10 "working on B leaves A's trash request pending" "$(field pendingTrashCount)" 1
capture trash-owner-focus-b
ctl setMonitorMode locked "$secondary"
wait_for "[[ \$(field pendingTrashCount) == 0 ]]" 10
equal E-34-10 "changing the lock cancels the retired owner's request" "$(field pendingTrashCount)" 0
expect_out E-34-10 "cancellation leaves the selected file untouched" "cat $case_dir/keep.txt" keep
capture trash-owner-retired
ctl setRoot "$original_root"

close_edges
ctl setMonitorMode active ""
focus_output "$primary"
ctl openBlade left
focus_output "$secondary"
ctl openBlade right
await_shape E-34-09 "separate Active owners exist before restart" '[1,0,0,1]'
focus_output "$primary"
sleep 1
restart_shell || abort "restart" "shell IPC did not return"
await_shape E-34-09 "restored open Active edges adopt initial focused output" '[1,1,0,0]'
equal E-34-09 "restart preserves the shared layout" "$(layout_content)" "$baseline_layout"
equal E-34-09 "restored left records A" "$(owner left)" "$primary"
equal E-34-09 "restored right records A" "$(owner right)" "$primary"
close_edges
ctl setMonitorMode locked "$secondary"
ctl openBlade left
sleep 1
restart_shell || abort "locked restart" "shell IPC did not return"
await_shape E-34-09 "locked output survives shell restart" '[0,0,1,0]'
equal E-34-09 "the lock survives shell restart" "$(field monitorLock)" "$secondary"
capture restored-lock
pending E-34-11 "Settings exposes and applies detected monitor choices" "requires dropdown pointer acceptance"
pending E-34-12 "startup and delayed focus cannot select a different owner" "requires deterministic QML coverage"
summary
