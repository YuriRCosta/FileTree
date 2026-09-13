#!/usr/bin/env bash
set -euo pipefail

: "${OVM:?set OVM to the native harness executable}"
: "${OVM_HOME:?set OVM_HOME for harness A}"
: "${OVM_SSH_PORT:?set OVM_SSH_PORT for harness A}"
: "${FILEBLADE_SHAPE:?set FILEBLADE_SHAPE=native}"
: "${SKIP_PUSH:?set SKIP_PUSH=1}"
[[ $FILEBLADE_SHAPE == native ]] || { printf '96-native-media-timeline: FILEBLADE_SHAPE must be native\n' >&2; exit 2; }
[[ $SKIP_PUSH == 1 ]] || { printf '96-native-media-timeline: SKIP_PUSH must be 1\n' >&2; exit 2; }

source "$(dirname "$0")/lib.sh"
require_guest

layout_path=$(field bladeLayoutPath)
[[ $layout_path == */blades.json ]] || { fail harness "native blades.json path" "$layout_path"; summary; }
read_layout() {
  local quoted
  printf -v quoted '%q' "$layout_path"
  guest "cat -- $quoted"
}

original_layout=$(read_layout)
original_left_slots=$(jq -c '.blades.left.slots' <<<"$original_layout")
original_right_slots=$(jq -c '.blades.right.slots' <<<"$original_layout")
original_left_width=$(jq -r '.blades.left.width' <<<"$original_layout")
original_right_width=$(jq -r '.blades.right.width' <<<"$original_layout")
original_left_open=$(jq -r '.blades.left.open' <<<"$original_layout")
original_right_open=$(jq -r '.blades.right.open' <<<"$original_layout")
original_right_notes=$(jq -c '[.blades.right.slots[]?.modules[]? | select(.module == "notes") | .state] | first' <<<"$original_layout")
original_root=$(field rootPath)
fixture_path=''
slot_id=r96-files
test_slots='[{"id":"r96-files","fraction":-1,"active":0,"collapsed":false,"modules":[{"module":"files","state":{"mediaMode":true,"mediaRecursive":false,"mediaQuery":"","mediaQueryReady":true,"mediaSizeStep":2,"ordinaryDensityStep":2,"summaryInTree":false}}]}]'

set_slots() {
  local edge=$1 slots=$2 encoded
  encoded=$(printf '%s' "$slots" | base64 -w0)
  ctl setBladeSlots "$edge" "base64:$encoded"
}

restore() {
  set +e
  if [[ -n ${original_left_slots:-} ]]; then
    set_slots left "$original_left_slots"
    set_slots right "$original_right_slots"
    ctl setBladeWidth left "$original_left_width"
    ctl setBladeWidth right "$original_right_width"
    if [[ $original_left_open == true ]]; then ctl openBlade left; else ctl closeBlade left; fi
    if [[ $original_right_open == true ]]; then ctl openBlade right; else ctl closeBlade right; fi
    ctl setRoot "$original_root"
    sleep 1
  fi
  if [[ -n ${fixture_path:-} ]]; then
    local quoted
    printf -v quoted '%q' "$fixture_path"
    guest "rm -rf -- $quoted" >/dev/null
  fi
}
trap restore EXIT

fixture_path=$(guest 'mktemp -d /tmp/fileblade-r96-native.XXXXXXXX')
[[ $fixture_path =~ ^/tmp/fileblade-r96-native\.[A-Za-z0-9]+$ ]] || { fail harness "fixture path" "$fixture_path"; summary; }
guest 'test -s /home/omarchy/fbexp/small.png'
quoted_fixture=$(printf '%q' "$fixture_path")
guest "cp -- /home/omarchy/fbexp/small.png $quoted_fixture/2020-01.png
cp -- /home/omarchy/fbexp/small.png $quoted_fixture/2026-12.png
touch -d '2020-01-15 12:00:00 UTC' $quoted_fixture/2020-01.png
touch -d '2026-12-15 12:00:00 UTC' $quoted_fixture/2026-12.png"

if thumbnail_result=$(backend thumbnail --path "$fixture_path/2020-01.png" --key r96 --width 120 --height 120); then
  thumbnail_path=$(jq -r '.path // empty' <<<"$thumbnail_result")
  thumbnail_size=0
  if [[ $thumbnail_path == */*.png ]]; then
    thumbnail_size=$(guest "stat -c %s -- $(printf '%q' "$thumbnail_path")" || true)
  fi
  if jq -e '.ok == true and (.path | endswith(".png"))' <<<"$thumbnail_result" >/dev/null && [[ $thumbnail_size =~ ^[1-9][0-9]*$ ]]; then
    pass E-37-01 'installed thumbnail backend returns a cached PNG'
  else
    fail E-37-01 'installed thumbnail backend returns a cached PNG' "$thumbnail_result size=$thumbnail_size"
  fi
else
  fail E-37-01 'installed thumbnail backend returns a cached PNG' 'thumbnail command failed'
fi

set_slots left "$test_slots"
ctl setBladeWidth left 380
ctl closeBlade right
ctl openBlade left
ctl focusBlade left
ctl setRoot "$fixture_path"
sleep 2

capture() {
  local name=$1
  last_shot=$( "$OVM" shot "r96-native-media-timeline-$name" 2>/dev/null | tail -1)
  [[ -f $last_shot ]] || { fail harness "capture $name" "$last_shot"; return 1; }
}

state_matches() {
  read_layout | jq -e --arg id "$slot_id" --argjson wanted "$1" '
    [.blades.left.slots[]? | select(.id == $id) | .modules[]? | select(.module == "files") | .state]
    | first as $state
    | ($state.mediaMode == true and $state.mediaQuery == "" and (($state.mediaShowEmptyPeriods // false) == $wanted))
  ' >/dev/null
}

wait_state() {
  local wanted=$1 deadline=$((SECONDS + ${2:-12}))
  while ((SECONDS < deadline)); do
    if state_matches "$wanted"; then return 0; fi
    sleep 0.5
  done
  return 1
}

capture default-omit
if wait_state false; then
  pass E-37-05 'fresh Files media state keeps media mode, empty query, and empty periods off'
else
  fail E-37-05 'fresh Files media state' 'saved state did not contain mediaMode=true, mediaQuery="", and mediaShowEmptyPeriods=false'
fi

ctl toggleBladeSettings left
sleep 1
settings_visible=$(screen_text 2>/dev/null | tr '[:lower:]' '[:upper:]' || true)
if [[ $settings_visible != *SETTINGS* ]]; then
  ctl toggleBladeSettings left
  sleep 1
fi
capture settings-open
settings_text=$(screen_text 2>/dev/null || true)
expect_contains E-37-05 'Media settings exposes the timeline option' "$settings_text" 'Show empty timeline periods'
"$OVM" mouse click 120 116
"$OVM" key ctrl-a
"$OVM" type 'Show empty timeline periods'
sleep 1
capture settings-filtered

locate_toggle_y() {
  local image=$1 crop result
  crop=$(mktemp --suffix=.png)
  if ! magick "$image" -crop '378x900+0+60' +repage -colorspace gray -level 5%,40% -negate -resize 300% "$crop" 2>/dev/null; then
    rm -f -- "$crop"
    return 1
  fi
  result=$(tesseract "$crop" - --psm 6 tsv 2>/dev/null | awk -F '\t' '$1 == 5 { y = 60 + ($8 + $10 / 2) / 3; if (y > 150 && toupper($12) ~ /^SHOW$/) { print int(y); exit } }')
  rm -f -- "$crop"
  [[ $result =~ ^[0-9]+$ ]] || return 1
  printf '%s\n' "$result"
}

if ! toggle_y=$(locate_toggle_y "$last_shot"); then
  fail E-37-05 'settings toggle pointer target' 'Show empty timeline periods row was not located by OCR'
  summary
fi
"$OVM" mouse click 100 "$toggle_y"
sleep 1
capture setting-on
if wait_state true; then
  pass E-37-05 'settings toggle saves show empty timeline periods'
else
  fail E-37-05 'settings toggle persistence state' 'mediaShowEmptyPeriods=true was not saved'
fi
ctl toggleBladeSettings left
sleep 1
capture empty-periods-shown

if ! restart_shell; then
  fail harness 'native restart' 'installed app did not restart'
  summary
fi
open_left
capture persisted-after-restart
if wait_state true; then
  pass E-37-05 'Files media setting and mode survive native restart'
else
  fail E-37-05 'Files media setting after restart' 'saved media state was not restored'
fi

layout_matches() {
  local restored
  restored=$(read_layout)
  jq -e --argjson left_open "$original_left_open" --argjson right_open "$original_right_open" \
    --argjson right_notes "$original_right_notes" \
    --arg left_width "$original_left_width" --arg right_width "$original_right_width" '
    .blades.left.open == $left_open
    and .blades.right.open == $right_open
    and .blades.left.width == ($left_width | tonumber)
    and .blades.right.width == ($right_width | tonumber)
    and ([.blades.right.slots[]?.modules[]? | select(.module == "notes") | .state] | first) == $right_notes
  ' <<<"$restored" >/dev/null && [[ $(field rootPath) == "$original_root" ]]
}

restore
trap - EXIT
pending E-37-05 'exact visible bins, dates and viewport geometry' 'inspect the default-omit and empty-periods-shown guest screenshots'
if wait_for layout_matches 10; then
  pass harness 'saved blades.json slots restore including Notes state'
else
  fail harness 'saved blades.json restoration' 'restored view values or Notes state did not match the saved document'
fi
summary
