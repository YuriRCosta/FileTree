#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
ctl clearSearch; ctl setSearchOptions false false; sleep 2
focus_tree

"$OVM" key slash; sleep 2
"$OVM" type "alpha"; sleep 1
wait_for "[[ \$(field searchQuery) == alpha ]]" 15
expect E-09-01 "slash focuses the search field" searchQuery "alpha"
wait_for "[[ \$(field searchBusy) == false ]]" 15
hits=$(search_names)
expect_contains E-09-02 "a plain query filters the results" "$hits" "alpha.txt"
expect_missing E-09-02 "and drops what does not match" "$hits" "bravo.txt"

ctl clearSearch; sleep 2
expect E-09-03 "clearing restores the listing" searchQuery ""
expect_contains E-09-03 "and everything is back" "$(tree_names)" "bravo.txt"

ctl setSearchDeep true; ctl search "deep"; sleep 4
wait_for "[[ \$(field searchBusy) == false ]]" 25
expect_true E-09-04 "a deep search reports results" "[[ \$(status | jq '.searchResults') -ge 1 ]]"
ctl clearSearch; ctl setSearchDeep false; sleep 2

ctl setSearchOptions true false; ctl search "ALPHA"; sleep 3
sensitive=$(search_names)
ctl setSearchOptions false false; ctl search "ALPHA"; sleep 3
insensitive=$(search_names)
expect_true E-09-05 "case sensitivity changes the matches" "[[ '$sensitive' != '$insensitive' ]]"
ctl clearSearch; sleep 2

ctl setSearchOptions false true; ctl search "al.*txt"; sleep 3
expect_contains E-09-06 "a valid regex matches by pattern" "$(search_names)" "alpha.txt"

ctl search "[unclosed"; sleep 2
wait_for "[[ \$(field searchError) == 'Invalid regular expression' ]]" 15
expect E-09-07 "an invalid regex says so" searchError "Invalid regular expression"
expect_contains E-09-07 "and the message is on screen" "$(summary_text)" "Invalid regular"
ctl clearSearch; ctl setSearchOptions false false; sleep 2

ctl setSearchDeep true; ctl search "alpha"; sleep 4
wait_for "[[ \$(field searchBusy) == false ]]" 25
ctl clearSearch; ctl setSearchDeep false; sleep 2
focus_tree
"$OVM" key slash; sleep 1
"$OVM" key up; sleep 2
expect_true E-09-08 "search history is reachable with Up" "[[ -n \$(field searchQuery) ]]"
ctl clearSearch; "$OVM" key esc; sleep 1
open_left

ctl clearSearch; sleep 1
"$OVM" mouse move 200 400; sleep 1
"$OVM" mouse click 100 82; sleep 1
"$OVM" mouse move 210 300; sleep 0.3; "$OVM" mouse move 230 350; sleep 0.3; "$OVM" mouse move 150 500; sleep 1
"$OVM" type "alp"; sleep 1
wait_for "[[ \$(field searchQuery) == alp ]]" 10
expect E-09-09 "search keeps focus while the pointer moves inside the section" searchQuery "alp"

"$OVM" mouse move 150 850; sleep 0.5; "$OVM" mouse move 170 880; sleep 1
"$OVM" type "zz"; sleep 1
expect E-09-10 "pointer in another section takes focus from search" searchQuery "alp"
"$OVM" mouse move 200 400; sleep 0.5; "$OVM" mouse click 100 82; sleep 1
ctl toggleBladeFocus right >/dev/null; sleep 2
ctl focusSearch >/dev/null; sleep 1
"$OVM" mouse move 1700 500; sleep 0.5; "$OVM" mouse move 1720 540; sleep 2
"$OVM" type "q"; sleep 1
expect E-09-10 "pointer over the other blade takes focus from search" focusedBlade right
expect E-09-10 "and the typing does not reach search" searchQuery "alp"
"$OVM" ssh 'pkill -x foot' >/dev/null 2>&1; sleep 1
"$OVM" ssh 'setsid foot >/dev/null 2>&1 < /dev/null &' >/dev/null 2>&1
wait_for "[[ \$("$OVM" hypr clients 2>/dev/null | jq length) -ge 1 ]]" 20
ctl focusSearch >/dev/null; sleep 1
"$OVM" mouse move 200 400; sleep 0.5; "$OVM" mouse click 100 82; sleep 1
"$OVM" mouse move 900 500; sleep 0.5; "$OVM" mouse move 950 520; sleep 2
wait_for "[[ -z \$(field focusedBlade) ]]" 10
"$OVM" type "y"; sleep 1
expect E-09-10 "pointer over another window releases blade focus" focusedBlade ""
expect E-09-10 "and the typing does not reach search" searchQuery "alp"
"$OVM" ssh 'pkill -x foot' >/dev/null 2>&1; sleep 1

ctl focusSearch >/dev/null; sleep 1
"$OVM" mouse move 200 400; sleep 0.5; "$OVM" mouse click 100 82; sleep 1
ctl focusDirection right >/dev/null; sleep 2
"$OVM" type "w"; sleep 1
expect E-09-11 "keyboard focus direction leaves search for the right blade" focusedBlade right
expect E-09-11 "and the typing does not reach search" searchQuery "alp"
ctl toggleBladeFocus right >/dev/null; sleep 2
ctl clearSearch; sleep 1
open_left

pending E-09-12 "search option chips sit at the right edge when the field is empty" "chip geometry is not reported over IPC and OCR cannot measure alignment"

summary
