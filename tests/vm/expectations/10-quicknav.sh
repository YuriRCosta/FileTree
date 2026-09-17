#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
guest "cd $ROOT_DIR && zoxide add deep dest 2>/dev/null; true"
focus_tree

"$OVM" key shift-z; sleep 3
expect E-10-01 "Shift+Z opens quick navigation" quickNavActive true
quicknav_text() {
  local shot crop=/tmp/fb-qn-ocr.png
  shot=$("$OVM" shot ocr-quicknav 2>/dev/null | tail -1)
  [[ -f $shot ]] || return 0
  magick "$shot" -crop 520x300+0+55 +repage -colorspace gray -level 15%,40% -negate -resize 250% "$crop" 2>/dev/null || return 0
  tesseract "$crop" - --psm 6 2>/dev/null | tr -s '[:space:]' ' '
}
expect_contains E-10-01 "and it names itself" "$(quicknav_text)" "QUICK NAV"

before_matches=$(field searchResults)
"$OVM" type "deep"; sleep 3
after_matches=$(field searchResults)
expect_true E-10-02 "typing narrows the candidates" "[[ '$after_matches' -le '$before_matches' && '$after_matches' -ge 1 ]]"

wait_for "[[ \$(field searchBusy) == false ]]" 15
"$OVM" key ret
wait_for "[[ \$(field rootPath) == '$ROOT_DIR/deep' ]]" 15
expect E-10-03 "Enter moves the root" rootPath "$ROOT_DIR/deep"
expect E-10-03 "and quick navigation closes" quickNavActive false

goto_root "$ROOT_DIR"
focus_tree
"$OVM" key shift-z; sleep 3
before=$(field rootPath)
"$OVM" key esc; sleep 2
expect E-10-04 "escape leaves quick navigation" quickNavActive false
expect_true E-10-04 "and the root is unchanged" "[[ \$(field rootPath) == '$before' ]]"

summary
