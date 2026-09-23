#!/usr/bin/env bash
set -euo pipefail
OVM=${OVM:?set OVM to the headless VM harness executable}
PLUGIN=yuricosta.filetree
status() { "$OVM" ipc "$PLUGIN" status; }
ctl() { "$OVM" ipc "$PLUGIN.control" "$@" >/dev/null; }
expect() {
  [[ $2 == "$3" ]] || { printf 'FAIL %s: [%s], wanted [%s]\n' "$1" "$2" "$3"; exit 1; }
  printf 'ok   %s\n' "$1"
}
sinks() { "$OVM" ssh 'stat -c %s /tmp/filetree-focus-first.log /tmp/filetree-focus-second.log'; }

original_sinks=$(sinks)
expect monitor-mode-all "$(status | jq -r .monitorMode)" all
monitor=$("$OVM" hypr monitors | jq -r '.[0].name')
ctl closeBlade left
ctl closeBlade right
sleep 0.6
"$OVM" ssh "omarchy-shell $PLUGIN.control focusBlade left; omarchy-shell $PLUGIN.control focusBlade right" >/dev/null
sleep 1
expect latest-request-wins "$(status | jq -r .focusedBlade)" right
expect no-terminal-input "$(sinks)" "$original_sinks"

before_monitors=$("$OVM" hypr monitors | jq -c '[.[].name]')
"$OVM" ssh 'hyprctl output create headless' >/dev/null
sleep 1
secondary=$("$OVM" hypr monitors | jq -r --argjson before "$before_monitors" '.[] | .name as $name | select($before | index($name) | not) | .name')
[[ -n $secondary ]] || { echo 'FAIL no secondary monitor'; exit 1; }
trap '"$OVM" ssh "hyprctl output remove $secondary" >/dev/null' EXIT
ctl setRoot /home/omarchy
ctl focusBladeOn left "$monitor"
sleep 0.5
ctl focusBladeOn left "$secondary"
sleep 1
before=$(status | jq -r .selectedPath)
"$OVM" key j
sleep 0.6
after=$(status | jq -r .selectedPath)
expect same-edge-new-monitor-keeps-focus "$(status | jq -r .focusedBlade)" left
[[ $before != "$after" ]] || { echo 'FAIL secondary monitor did not receive navigation'; exit 1; }
echo 'ok   secondary monitor receives navigation'
expect no-terminal-input "$(sinks)" "$original_sinks"
ctl focusBladeOn left "$monitor"
sleep 0.5
"$OVM" key j
sleep 0.5
expect return-to-first-monitor-keeps-focus "$(status | jq -r .focusedBlade)" left
expect no-terminal-input "$(sinks)" "$original_sinks"
"$OVM" shot focus-ownership
