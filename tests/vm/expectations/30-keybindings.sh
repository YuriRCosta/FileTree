#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"
require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree
keymap=$(field keybindingsPath)
[[ $keymap == /home/omarchy/.config/omarchy/fileblade/keybindings.json ]] || { fail harness "keymap path" "$keymap"; summary; }
backup=$(guest "mktemp -d /home/omarchy/.config/omarchy/fileblade/.keybindings-test.XXXXXX") || { fail harness "keymap backup" "could not create backup directory"; summary; }
guest "if test -e '$keymap'; then cp -- '$keymap' '$backup/original'; fi"
restore_keymap() {
  guest "if test -f '$backup/original'; then cp -- '$backup/original' '$keymap'; else rm -f -- '$keymap'; fi"
  ctl reloadKeybindings >/dev/null
}
trap restore_keymap EXIT
write_keys() {
  guest "printf '%s\n' '$1' > '$backup/next' && mv -- '$backup/next' '$keymap'" || { fail harness "save keybindings" "atomic replacement failed"; summary; }
  sleep 2
}
expanded() { "$OVM" ipc "$PLUGIN" tree 300 | jq -r --arg p "$1" '[.entries[]|select(.path==$p)][0].expanded'; }
custom='{"version":1,"bindings":{"open":["F3"],"up":["F4"],"expand":["g o"],"collapse":["g c"],"help":["F1"]}}'
write_keys "$custom"
expect E-30-01 "valid bindings load without restarting the shell" keybindingsError ""
click_row deep
"$OVM" key l; sleep 1
expect E-30-01 "overridden l no longer enters a folder" rootPath "$ROOT_DIR"
"$OVM" key f3; sleep 2
expect E-30-01 "F3 enters the folder instead" rootPath "$ROOT_DIR/deep"
"$OVM" key f4; sleep 2
expect E-30-01 "F4 goes to its parent" rootPath "$ROOT_DIR"
click_row deep
"$OVM" key g; "$OVM" key o; sleep 2
expect_true E-30-02 "a custom sequence expands the folder" "[[ \$(expanded '$ROOT_DIR/deep') == true ]]"
"$OVM" key g; "$OVM" key c; sleep 1
expect_true E-30-02 "a custom sequence collapses the folder" "[[ \$(expanded '$ROOT_DIR/deep') == false ]]"
"$OVM" key g; "$OVM" key esc; sleep 1
expect E-30-02 "Escape cancels the sequence without closing" open true
"$OVM" key g
ctl focusProperties; sleep 1
focus_tree
"$OVM" key o; sleep 1
expect E-30-02 "focus loss cancels the sequence" rootPath "$ROOT_DIR"
expect_true E-30-02 "and the folder stays collapsed" "[[ \$(expanded '$ROOT_DIR/deep') == false ]]"

write_keys '{"bindings":{"open":[]}}'
"$OVM" key o; "$OVM" key l; "$OVM" key right; sleep 1
expect E-30-02 "an empty array disables every open binding" rootPath "$ROOT_DIR"
write_keys "$custom"
write_keys '{"bindings":{"open":["F1"],"help":["F1"]}}'
expect_contains E-30-03 "ambiguous custom bindings report an error" "$(field keybindingsError)" "Conflicting"
click_row deep
"$OVM" key f3; sleep 2
expect E-30-03 "and leave the last valid binding active" rootPath "$ROOT_DIR/deep"
"$OVM" key f4; sleep 2
write_keys '{not json'
expect_true E-30-03 "malformed JSON is rejected" "[[ -n \$(field keybindingsError) ]]"
write_keys '{}'
expect E-30-03 "fixing the config clears the error" keybindingsError ""
write_keys "$custom"
for module in git memory skills hooks mcp; do
  extension="data-goblin.fileblade-$module"
  if ! guest "test -f /home/omarchy/.config/omarchy/plugins/$extension/manifest.json"; then
    pending E-30-04 "$module inherits tree keys" "extension not installed in this guest"
    continue
  fi
  guest "$GUEST_PLUGIN/fileblade blade set right $extension/$module" >/dev/null
  ctl openBlade right
  ctl focusBlade right
  sleep 3
  "$OVM" key f1; sleep 2
  guide=$(ocr_crop "ocr-keybindings-$module" "360x1054+1560+26" 300% 6)
  expect_contains E-30-04 "$module inherits the remapped help key and open binding" "$guide" "F3"
  "$OVM" key esc; sleep 1
done
guest "$GUEST_PLUGIN/fileblade blade set right notes" >/dev/null
ctl focusBlade left
focus_tree
guest "rm -- '$keymap'"; sleep 2
click_row deep
"$OVM" key l; sleep 2
expect E-30-03 "removing the config restores the defaults" rootPath "$ROOT_DIR/deep"
"$OVM" key h; sleep 2
summary
