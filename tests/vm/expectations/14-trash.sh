#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree
guest "printf 'doomed\n' > $ROOT_DIR/doomed.txt"; sleep 3

click_row doomed.txt
"$OVM" key d; sleep 2.5
expect_contains E-14-01 "d asks before trashing" "$(screen_text)" "Trash"
expect_out E-14-01 "and nothing has moved yet" "test -f $ROOT_DIR/doomed.txt && echo yes || echo no" yes

"$OVM" key esc; sleep 2
expect_out E-14-02 "cancel leaves the file alone" "test -f $ROOT_DIR/doomed.txt && echo yes || echo no" yes

before=$(field trashCount)
click_row doomed.txt
"$OVM" key delete; sleep 2.5
"$OVM" key tab; sleep 1
"$OVM" key ret; sleep 4
expect_out E-14-03 "confirming moves the file off its path" "test -e $ROOT_DIR/doomed.txt && echo yes || echo no" no
expect_true E-14-03 "and the trash count rises" "[[ \$(field trashCount) -gt $before ]]"

ctl navigate "trash:///"; sleep 4
expect E-14-04 "the trash view opens" trashMode true
expect_contains E-14-04 "and lists the entry by its original name" "$(trash_names)" "doomed.txt"

"$OVM" mouse move 200 152; sleep 2
rowtext=$(ocr_crop trash-hover 378x30+0+138 300% 7)
expect_contains E-14-05 "the hovered row still names the entry" "$rowtext" "doomed"
expect_missing E-14-05 "and its date is dimmed rather than sliced" "$rowtext" "2026-"

"$OVM" mouse click "$(( $(field sidebarWidth) - 85 ))" 152; sleep 4
expect_out E-14-06 "restore puts the file back" "test -f $ROOT_DIR/doomed.txt && echo yes || echo no" yes

click_row_trash() { "$OVM" mouse move 200 152; sleep 1.5; }
goto_root "$ROOT_DIR"
focus_tree
click_row doomed.txt
"$OVM" key delete; sleep 2.5; "$OVM" key tab; sleep 1; "$OVM" key ret; sleep 4
ctl navigate "trash:///"; sleep 4
info=$(guest 'cat ~/.local/share/Trash/info/*.trashinfo 2>/dev/null | head -20')
expect_contains E-14-11 "the trashinfo names the original path" "$info" "$ROOT_DIR/doomed.txt"
gio_paths=$(guest 'gio list -a trash::orig-path trash:///')
expect_contains E-14-11 "gio resolves the trashed item to its original path" "$gio_paths" "$ROOT_DIR/doomed.txt"
expect_missing E-14-11 "gio does not retain an internal staging path" "$gio_paths" ".fileblade-stage-"

click_row_trash
"$OVM" mouse click "$(( $(field sidebarWidth) - 30 ))" 152; sleep 3
expect_contains E-14-07 "delete permanently asks first" "$(screen_text)" "permanently"
"$OVM" key tab; sleep 1.5
still=$(field trashCount)
expect_true E-14-08 "tab stays inside the dialog" "[[ \$(field trashCount) == '$still' ]]"
"$OVM" key esc; sleep 2
expect_true E-14-07 "escape cancels" "[[ \$(field trashCount) -ge 1 ]]"

click_row_trash
"$OVM" mouse click "$(( $(field sidebarWidth) - 30 ))" 152; sleep 3
"$OVM" key ret; sleep 4
expect E-14-09 "Enter confirms and the trash empties" trashCount 0
expect_out E-14-09 "and the file is gone from the trash directory" "ls ~/.local/share/Trash/files 2>/dev/null | wc -l" 0

goto_root "$ROOT_DIR"
focus_tree
guest "printf 'sweep\n' > $ROOT_DIR/sweep.txt"; sleep 3
click_row sweep.txt
"$OVM" key delete; sleep 2.5; "$OVM" key tab; sleep 1; "$OVM" key ret; sleep 4
ctl navigate "trash:///"; sleep 4
"$OVM" mouse click "$(( $(field sidebarWidth) - 25 ))" 115; sleep 3
expect_contains E-14-10 "empty trash asks first" "$(screen_text)" "Empty"
"$OVM" key ret; sleep 4
expect E-14-10 "and clears every entry" trashCount 0

goto_root "$ROOT_DIR"

goto_root "$ROOT_DIR"
focus_tree
guest "printf 'one\\n' > $ROOT_DIR/zz-one.txt; printf 'two\\n' > $ROOT_DIR/zz-two.txt"
wait_for "[[ \$(row_index zz-two.txt) != null ]]" 15
before=$(field trashCount)
click_row zz-one.txt
"$OVM" key delete; sleep 2.5; "$OVM" key tab; sleep 1; "$OVM" key ret
wait_for "[[ \$(field trashCount) -gt $before ]]" 15
click_row zz-two.txt
"$OVM" key delete; sleep 2.5; "$OVM" key tab; sleep 1; "$OVM" key ret
wait_for "[[ \$(field trashCount) -gt $((before + 1)) ]]" 15
ctl navigate "trash:///"; sleep 4
expect_true E-14-12 "precondition: two items sit in the trash" "[[ \$(field trashCount) -ge 2 ]]"
"$OVM" mouse click 150 152; sleep 1
first=$(field trashSelectedId)
"$OVM" key j; sleep 1
second=$(field trashSelectedId)
expect_true E-14-12 "j moves the trash selection" "[[ -n '$first' && -n '$second' && '$second' != '$first' ]]"
"$OVM" key k; sleep 1
expect E-14-12 "k returns to the first trash entry" trashSelectedId "$first"

summary
