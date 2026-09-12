#!/usr/bin/env bash
# Dragging files outside a blade. Expectations E-26-01 .. E-26-10.
source "$(dirname "$0")/lib.sh"

require_guest

wheel() { status | jq -r ".dropWheel.$1"; }
wheel_labels() { status | jq -r '[.dropWheel.actions[]?.label]|join(",")'; }
wheel_index() { status | jq -r --arg n "$1" '[.dropWheel.actions[]?.label]|index($n)'; }
clients() { "$OVM" hypr clients 2>/dev/null | jq length; }
kill_windows() { "$OVM" ssh 'pkill -x foot; pkill -x nvim' >/dev/null 2>&1; wait_for "[[ \$(clients) == 0 ]]" 15; }
# Wedge i of n is centred at -90 + i * 360 / n degrees, on the ring between the
# hub and the rim (WheelGeometry.wedgeAngle; labelRadius is 58 at scale 1).
wedge_point() { awk -v x="$1" -v y="$2" -v n="$3" -v i="$4" 'BEGIN { a = (-90 + i * 360 / n) * 3.14159265 / 180; printf "%d %d\n", x + 58 * cos(a), y + 58 * sin(a) }'; }

# The drag is a democtl script inside the guest, a real pointer from press to
# release. Space is held by demos/hold-space-near.sh from the pushed plugin: it
# waits until the pointer comes within REACH px of the drop point, then holds
# Space for HOLD ms. A small reach on an eased path opens the wheel where the
# drag ends, so the release lands on the hub; a large reach on a linear path
# opens it REACH px early, so the release lands on the wedge in the direction
# of travel. While the recording runs the host polls the status document and
# keeps two snapshots: the first that shows the wheel open, for the drag facts,
# and the first that shows its rows loaded, for the target; the backend answers
# a moment after the wheel appears, often after the release.
drag_with_space() {
  local sx=$1 sy=$2 tx=$3 ty=$4 ease=$5 reach=$6 hold=$7 script
  script=$(cat <<TOML
[demo]
name = "expect-wheel"
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
source = [$sx, $sy]
target = [$tx, $ty]

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
ease = "$ease"
ms = 3000

[[step]]
kind = "hold"
ms = 1500
TOML
)
  guest "printf '%s\n' $(printf '%q' "$script") > /tmp/fb-wheel.toml"
  guest "setsid $GUEST_PLUGIN/demos/hold-space-near.sh $tx $ty $reach $hold >/tmp/fb-wheel-space.log 2>&1 </dev/null &" >/dev/null
  guest 'setsid democtl record /tmp/fb-wheel.toml --out /tmp --force >/tmp/fb-wheel-record.log 2>&1 </dev/null &' >/dev/null
  mid_drag=""; loaded=""
  local deadline=$((SECONDS + 12)) snapshot
  while ((SECONDS < deadline)); do
    snapshot=$(status | jq -c '.dropWheel' 2>/dev/null)
    if [[ $(jq -r '.open' <<<"$snapshot" 2>/dev/null) == true ]]; then
      [[ -z $mid_drag ]] && mid_drag=$snapshot
      if [[ $(jq -r '.loading | not' <<<"$snapshot" 2>/dev/null) == true ]]; then loaded=$snapshot; break; fi
    fi
    sleep 0.2
  done
  wait_for "! guest 'pgrep -x democtl' | grep -q ." 30
  sleep 1.5
}
mid() { jq -r ".$1" <<<"${mid_drag:-null}" 2>/dev/null; }
seen() { jq -r ".$1" <<<"${loaded:-null}" 2>/dev/null; }

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
kill_windows
focus_tree
ctl hideDropWheel >/dev/null 2>&1

# Release on the hub: the wheel must open under the pointer and then stay.
drag_with_space "$ROW_X" "$(row_y "$(row_index alpha.txt)")" 900 500 ease_in_out 12 3000
expect_true E-26-01 "leaving the blade with a row starts a drag" "[[ \$(mid dragging) == true ]]"
expect_true E-26-01 "that carries one item" "[[ \$(mid count) == 1 ]]"
pending E-26-02 "the ghost names the last grabbed row and counts the items" "the ghost caption is not in the status document; the carried count is covered by E-26-01"
expect_true E-26-03 "holding the modifier opens the wheel during the drag" "[[ \$(mid open) == true && \$(mid fromDrag) == true ]]"
expect_true E-26-03 "at the pointer" "[[ \$(mid x) -gt 860 && \$(mid x) -lt 940 && \$(mid y) -gt 460 && \$(mid y) -lt 540 ]]"
labels=$(jq -r '[.actions[]?.label]|join(",")' <<<"${loaded:-null}" 2>/dev/null)
expect_true E-26-04 "over the bare desktop the target is the desktop" "[[ \$(seen target) == Desktop ]]"
expect_contains E-26-04 "which offers the default open" "$labels" "Open in new window"
expect_contains E-26-04 "and a new terminal" "$labels" "New terminal"
expect_missing E-26-04 "but nothing that needs a window under the pointer" "$labels" "herdr"
pending E-26-06 "placements open a second ring" "the second ring's entries are not in the status document"
expect_true E-26-09 "releasing on the hub keeps the wheel open" "[[ \$(wheel open) == true && \$(wheel dragging) == false ]]"
expect_true E-26-09 "as a wheel that no longer follows a drag" "[[ \$(wheel fromDrag) == false ]]"
"$OVM" shot wheel-26-hub

count=$(status | jq '.dropWheel.actions|length')
wx=$(wheel x); wy=$(wheel y)
terminal_index=$(wheel_index "New terminal")
if [[ $count -gt 0 && $terminal_index != null ]]; then
  read -r px py <<<"$(wedge_point "$wx" "$wy" "$count" "$terminal_index")"
  "$OVM" mouse move "$px" "$py"; sleep 1.5
  expect_true E-26-05 "the pointer highlights the wedge under it" "[[ \$(wheel highlighted) == $terminal_index ]]"
  "$OVM" mouse move "$wx" "$wy"; sleep 1.2
  expect_true E-26-05 "and the hub highlights nothing" "[[ \$(wheel highlighted) == -1 ]]"
else
  fail E-26-05 "the pointer highlights the wedge under it" "no wheel stayed open to move around in"
fi
"$OVM" key esc; sleep 1.5
expect_true E-26-08 "escape closes the wheel" "[[ \$(wheel open) == false ]]"
expect_true E-26-08 "without opening anything" "[[ \$(clients) == 0 ]]"
expect_out E-26-08 "and the file is untouched" "test -f $ROOT_DIR/alpha.txt && echo yes || echo no" yes

# Release on a wedge: the wheel opens 60 px before the drop, so the release
# lands on the wedge in the direction of travel. From a low row up and right
# the angle is about -42 degrees, inside the top wedge, Open in new window.
drag_with_space "$ROW_X" "$(row_y "$(row_index long.txt)")" 560 220 linear 60 3000
wait_for "[[ \$(clients) -ge 1 ]]" 20
expect_true E-26-07 "releasing on Open in new window opens the file" "[[ \$(clients) -ge 1 ]]"
expect_true E-26-07 "and the wheel closes" "[[ \$(wheel open) == false ]]"
"$OVM" shot wheel-26-opened
kill_windows

pending E-26-10 "the Open with wedge shows an open-folder glyph in every context" "wedge glyphs are not in the status document"

summary
