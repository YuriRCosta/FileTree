#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

STATE=$(field bladeLayoutPath)
original_slots=$(guest "jq -c .blades.right.slots $(printf '%q' "$STATE")")
if ! jq -e 'type == "array"' <<<"$original_slots" >/dev/null; then
  fail E-25-01 "layout backup" "cannot read the saved blade slots"
  summary
fi
original_open=$("$OVM" ipc "$PLUGIN" blades | jq -r .blades.right.open)
original_focus=$(field focusedBlade)
cleanup() {
  ctl setBladeSlots right "base64:$(printf '%s' "$original_slots" | base64 -w0)"
  [[ $original_open == true ]] || ctl closeBlade right
  if [[ -n $original_focus ]]; then ctl focusBlade "$original_focus"; else ctl releaseBladeFocus; fi
}
trap cleanup EXIT
notes_state() {
  guest "jq -c '.blades.right.slots[] | select(.id == \"expectation-notes\") | .modules[] | select(.module == \"notes\") | .state.text' $(printf '%q' "$STATE")"
}
note_value() { notes_state | jq -r "$1"; }

ctl setBladeSlots right 'base64:W10='
sleep 1
ctl setBladeSlots right "base64:$(printf '%s' '[{"id":"expectation-notes","module":"notes"}]' | base64 -w0)"
ctl focusBlade right
guest 'omarchy-shell notifications dismissAll' >/dev/null
sleep 1
expect_true E-25-01 "Notes loads in the requested blade" '[[ $("$OVM" ipc "$PLUGIN" blades | jq -r .blades.right.slots[0].modules[0].module) == notes ]]'
"$OVM" shot notes-before >/dev/null

"$OVM" key ctrl-a
"$OVM" type "expectation note one"
wait_for '[[ $(note_value ".items[0].text") == "expectation note one" ]]' 15
expect_true E-25-01 "typing reaches the note" '[[ $(note_value ".items[0].text") == "expectation note one" ]]'
expect_true E-25-02 "idle saves without a Save action" '[[ $(note_value ".revision") -gt 0 ]]'

notes_x=$((1920 - $(field propertiesBladeWidth) + 4))
"$OVM" mouse click $((notes_x + 76)) 78
sleep 1
expect_true E-25-03 "the + control adds a named note" '[[ $(note_value ".items | length") == 2 && $(note_value ".items[1].label") == "Note 2" ]]'
"$OVM" type "expectation note two"
sleep 1
expect_true E-25-04 "each note keeps its own text" '[[ $(note_value ".items[0].text") == "expectation note one" && $(note_value ".items[1].text") == "expectation note two" ]]'
"$OVM" mouse click $((notes_x + 20)) 78
sleep 1
expect_true E-25-04 "selecting a tab restores its active identity" '[[ $(note_value .activeId) == $(note_value ".items[0].id") ]]'
double_click $((notes_x + 20)) 78
"$OVM" type "Release notes"
"$OVM" key ret
sleep 1
expect_true E-25-05 "double-clicking renames a note" '[[ $(note_value ".items[0].label") == "Release notes" ]]'
"$OVM" shot notes-tabs >/dev/null

restart_shell
guest 'omarchy-shell notifications dismissAll' >/dev/null
ctl focusBlade right
expect_true E-25-07 "both notes and their names survive restart" '[[ $(note_value ".items[0].label") == "Release notes" && $(note_value ".items[1].text") == "expectation note two" ]]'
"$OVM" mouse mclick $((notes_x + 20)) 78
sleep 1
expect_true E-25-06 "middle-click closes one note" '[[ $(note_value ".items | length") == 1 ]]'
"$OVM" mouse mclick $((notes_x + 20)) 78
sleep 1
expect_true E-25-06 "the final note cannot be closed" '[[ $(note_value ".items | length") == 1 ]]'

"$OVM" mouse click $((notes_x + 76)) 78
sleep 1
expect_true E-25-03 "new notes keep increasing after deletion" '[[ $(note_value ".items[0].label") == "Note 2" && $(note_value ".items[1].label") == "Note 3" ]]'
"$OVM" mouse mclick $((notes_x + 85)) 78
sleep 1

ctl focusBlade right
"$OVM" key ctrl-a
guest 'head -c 66000 /dev/zero | tr "\0" x | wl-copy >/dev/null 2>&1'
"$OVM" key ctrl-v
sleep 3
expect_true E-25-08 "an oversized paste is clamped at the shared limit" '[[ $(note_value ".items[0].text | length") == 65536 ]]'
expect_contains E-25-08 "the limit is explained" "$(screen_text)" "limit"
"$OVM" shot notes-limit >/dev/null

summary
