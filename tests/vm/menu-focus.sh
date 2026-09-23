#!/bin/bash
# Live check of the right-click menu in the headless Omarchy VM (test-omarchy-plugin).
# Usage: OVM=<headless VM harness> tests/vm/menu-focus.sh
# Assumes the guest is up, the plugin pushed, the shell restarted, and a file row at ROW_X,ROW_Y
# with a window under the pointer beyond the blade layer at OUT_X,OUT_Y.
set -u
OVM=${OVM:?set OVM to the ovm harness path}
ROW_X=${ROW_X:-95}; ROW_Y=${ROW_Y:-182}
FIRST_ROW_X=${FIRST_ROW_X:-152}; FIRST_ROW_Y=${FIRST_ROW_Y:-281}
IN_LAYER_X=${IN_LAYER_X:-900}; IN_LAYER_Y=${IN_LAYER_Y:-600}
OUT_X=${OUT_X:-1500}; OUT_Y=${OUT_Y:-600}
fails=0

status() { "$OVM" ipc yuricosta.filetree status 2>/dev/null; }
field() { status | jq -r ".$1"; }
expect() {
  local name=$1 key=$2 want=$3 got
  got=$(field "$key")
  if [[ $got == "$want" ]]; then echo "ok   $name: $key=$got"; else echo "FAIL $name: $key=$got wanted $want"; fails=$((fails + 1)); fi
}
reset() {
  [[ $(field actionMenuOpen) == true ]] && "$OVM" ipc yuricosta.filetree.control hideActions >/dev/null
  if [[ $(field open) != true || $(field focusedBlade) != left ]]; then
    "$OVM" ipc yuricosta.filetree.control toggleBladeFocus left >/dev/null
    sleep 1.5
  fi
  "$OVM" mouse click "$ROW_X" "$ROW_Y"; sleep 0.5
}

reset
echo "== keyboard: right-click, type ren, Enter, Esc"
"$OVM" mouse rclick "$ROW_X" "$ROW_Y"; sleep 1.2
expect open actionMenuOpen true
expect right-click-keeps-focus focusedBlade left
"$OVM" type "ren"; "$OVM" key ret; sleep 1
expect rename-by-key actionMenuMode rename
expect menu-keyboard-keeps-focus focusedBlade left
"$OVM" key esc; sleep 0.6
expect esc-closes actionMenuOpen false
expect esc-keeps-blade open true
expect esc-keeps-focus focusedBlade left

reset
echo "== pointer: right-click, type ren, click the first row"
"$OVM" mouse rclick "$ROW_X" "$ROW_Y"; sleep 1.2
"$OVM" type "ren"; sleep 1
"$OVM" mouse click "$FIRST_ROW_X" "$FIRST_ROW_Y"; sleep 1
expect rename-by-click actionMenuMode rename
"$OVM" key esc; sleep 0.6

"$OVM" ipc yuricosta.filetree.control clearClipboard >/dev/null
reset
echo "== pointer: right-click, filter copy, click the Copy row"
expect copy-starts-empty clipboardCount 0
"$OVM" mouse rclick "$ROW_X" "$ROW_Y"; sleep 1.2
"$OVM" type "copy"; sleep 1
"$OVM" mouse click "$FIRST_ROW_X" "$FIRST_ROW_Y"; sleep 1
expect copy-row-runs clipboardCount 1
expect copy-row-closes actionMenuOpen false

reset
echo "== keyboard-opened menu: m, type ren, Enter"
"$OVM" key m; sleep 1.2
expect m-opens actionMenuOpen true
"$OVM" type "ren"; "$OVM" key ret; sleep 1
expect rename-from-m actionMenuMode rename
"$OVM" key esc; sleep 0.6

reset
echo "== pointer parked over a window inside the layer keeps the menu and the blade focus"
"$OVM" mouse rclick "$ROW_X" "$ROW_Y"; sleep 1.2
"$OVM" mouse move "$IN_LAYER_X" "$IN_LAYER_Y"; sleep 1.8
expect parked-menu actionMenuOpen true
expect parked-focus focusedBlade left
"$OVM" key esc; sleep 0.6

reset
echo "== click on a window beyond the layer closes the menu"
"$OVM" mouse rclick "$ROW_X" "$ROW_Y"; sleep 1.2
"$OVM" mouse click "$OUT_X" "$OUT_Y"; sleep 1
expect outside-click-closes actionMenuOpen false

reset
echo "== click in the tree closes the menu and keeps the blade"
"$OVM" mouse rclick "$ROW_X" "$ROW_Y"; sleep 1.2
"$OVM" mouse click "$ROW_X" $((ROW_Y + 30)); sleep 1
expect tree-click-closes actionMenuOpen false
expect tree-click-keeps-focus focusedBlade left

"$OVM" shot menu-focus
echo "failures: $fails"
exit $((fails > 0))
