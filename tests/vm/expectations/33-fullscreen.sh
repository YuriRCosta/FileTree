#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"
set -e
require_guest
wait_for 'status >/dev/null' 20
repo=$(cd "$(dirname "$0")/../../.." && pwd)
initial=$("$OVM" ipc "$PLUGIN" blades)
work=$(guest 'mktemp -d /tmp/fileblade-fullscreen.XXXXXX')
browser_pid=""
shots=()
cleanup() {
  if [[ $browser_pid =~ ^[0-9]+$ ]]; then guest "kill '$browser_pid'" >/dev/null 2>&1 || true; fi
  for edge in left right; do
    if [[ $(jq -r ".blades.$edge.open" <<<"$initial") == true ]]; then ctl openBlade "$edge"; else ctl closeBlade "$edge"; fi
  done
  guest "rm -rf -- '$work'" >/dev/null 2>&1 || true
  if [[ ${KEEP_SHOTS:-0} != 1 && ${#shots[@]} -gt 0 ]]; then rm -f -- "${shots[@]}"; fi
}
trap cleanup EXIT
payload=$(base64 -w0 "$repo/tests/vm/fixtures/fullscreen-video.html")
guest "printf %s '$payload' | base64 -d > '$work/video.html'"
ctl openBlade left
ctl openBlade right
ctl releaseBladeFocus
browser_pid=$(guest "setsid chromium --ozone-platform=wayland --user-data-dir='$work/profile' --no-first-run --no-default-browser-check --disable-extensions --app='file://$work/video.html' > '$work/browser.log' 2>&1 < /dev/null & echo \$!")
window() { "$OVM" hypr clients | jq -c '[.[]|select(.title=="FileTree fullscreen regression")][0] // {}'; }
mode() { window | jq -r '.fullscreen // -1'; }
wait_for '[[ $(mode) == 0 ]]' 30
monitor=$("$OVM" hypr monitors | jq -c '.[0]')
width=$(jq -r '.width / .scale | floor' <<<"$monitor")
height=$(jq -r '.height / .scale | floor' <<<"$monitor")
blade_state=$("$OVM" ipc "$PLUGIN" blades | jq -c '.blades')
fits() {
  local win bounds
  win=$(window)
  bounds=$("$OVM" hypr monitors | jq -c '.[0]')
  jq -e --argjson m "$bounds" '.at[0] >= ($m.x + $m.reserved[0]) and (.at[0] + .size[0]) <= ($m.x + $m.width / $m.scale - $m.reserved[2])' <<<"$win" >/dev/null
}
image_check() {
  local label=$1 fullscreen=$2 shot x green
  sleep 2
  shot=$("$OVM" shot "fullscreen-$label" | tail -1)
  shots+=("$shot")
  for x in 20 $((width-20)); do
    green=$(magick "$shot" -crop "1x1+$x+$((height/2))" +repage -format '%[fx:abs(r-21/255)+abs(g-148/255)+abs(b-71/255)<0.04?1:0]' info:)
    expect_out E-21-01 "$label: video covers former sidebar area at x=$x only in fullscreen" "printf '%s' '$green'" "$fullscreen"
  done
  printf 'Screenshot: %s\n' "$shot"
}
focus_video() {
  local win x y
  win=$(window)
  x=$(jq -r '.at[0]+(.size[0]/2|floor)' <<<"$win")
  y=$(jq -r '.at[1]+(.size[1]/2|floor)' <<<"$win")
  if [[ ${1:-} == double ]]; then
    local output
    output=$(jq -r .name <<<"$monitor")
    guest "democtl click --output '$output' '$x' '$y' && democtl click --output '$output' '$x' '$y'"
  else
    "$OVM" mouse click "$x" "$y"
  fi
}
focus_video
"$OVM" key e
expect_true E-21-01 "video expanded within its window respects both sidebars" fits
image_check in-window 0
"$OVM" mouse move 100 150
ctl focusBlade left
wait_for '[[ $(field focusedBlade) == left ]]' 15
"$OVM" key meta_l-f
wait_for '[[ $(mode) == 2 ]]' 15
wait_for '[[ -z $(field focusedBlade) ]]' 15
expect E-21-01 "entering fullscreen releases the hidden sidebar's keyboard focus" focusedBlade ""
image_check super-f 1
"$OVM" key meta_l-f
wait_for '[[ $(mode) == 0 ]]' 15
wait_for fits 15
image_check restored 0
guest 'hyprctl dispatch "hl.dsp.window.fullscreen({ mode = \"maximized\" })"' >/dev/null
wait_for '[[ $(mode) == 1 ]]' 15
wait_for fits 15
expect_true E-21-01 "maximizing respects both sidebars" fits
focus_video double
wait_for '[[ $(mode) == 2 ]]' 15
image_check video-double-click 1
"$OVM" key esc
wait_for '[[ $(mode) == 0 || $(mode) == 1 ]]' 15
wait_for fits 15
image_check video-exit 0
expect_true E-21-01 "fullscreen preserves sidebar widths, sections, and open state" '[[ $("$OVM" ipc "$PLUGIN" blades | jq -c .blades) == "$blade_state" ]]'
summary
