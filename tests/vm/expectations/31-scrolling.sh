#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

RULER_X=366
RULER_TOP=$(row_y 0)
ruler_strip() { "$OVM" shot "$1" 2>/dev/null | tail -1; }
ruler_pixels() {
  local shot=$1 colour=$2
  magick "$shot" -crop "8x860+361+$((RULER_TOP + 40))" +repage -fuzz 6% -fill white -opaque "$colour" -fill black +opaque white -format '%[fx:int(mean*w*h)]' info: 2>/dev/null
}
tree_view() { local shot; shot=$("$OVM" shot "$1" 2>/dev/null | tail -1); magick "$shot" -crop "300x540+0+$RULER_TOP" +repage "${shot%.png}-view.png" && printf '%s\n' "${shot%.png}-view.png"; }
same_view() { local diff; diff=$(magick compare -metric AE -fuzz 4% "$1" "$2" null: 2>&1); diff=${diff%% *}; diff=${diff%%.*}; [[ $diff =~ ^[0-9]+$ ]] && (( diff < 400 )); }

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
guest "cd $ROOT_DIR && for i in 11 12 13 14 15 17 19 21 22 23 24 25 27 29 31 32 33 34 35 37 39 41 42 43 44 45 47 49 51 52 53 54 55 57 59 71 72 73 74 75 77 79 91 92 93 94 95 97 99; do printf x > many-\$i.txt; done"
sleep 3
ctl expandPath "$ROOT_DIR/repo"; sleep 2
focus_tree
"$OVM" key end; sleep 1.5

expect_true E-31-01 "the tree scrolled to its end" "[[ '$(visible_row_y many-57.txt)' =~ ^[0-9]+$ ]]"
before=$(tree_view scroll-end)
guest "printf 'more\n' >> $ROOT_DIR/many-21.txt"; sleep 4
expect_true E-31-01 "a file change in the viewed directory keeps the rows in place" "same_view '$before' '$(tree_view after-change)'"
guest "printf 'new\n' > $ROOT_DIR/many-12b.txt"; sleep 4
expect_true E-31-01 "a new file above the viewport keeps the rows in place" "same_view '$before' '$(tree_view after-create)'"
expect_contains E-31-01 "and the new file is listed" "$(tree_names)" "many-12b.txt"

ctl refresh; sleep 5
expect_true E-31-02 "Shift+R keeps the rows in place" "same_view '$before' '$(tree_view after-refresh)'"
expect_true E-31-02 "and the expanded repository survives" "[[ \$(tree_names) == *tracked.txt* ]]"

shot=$(ruler_strip ruler-bottom)
thumb=$(ruler_pixels "$shot" '#607DBD')
expect_true E-31-03 "the ruler thumb is drawn while the tree overflows" "[[ '$thumb' =~ ^[0-9]+$ && '$thumb' -ge 40 ]]"
"$OVM" mouse click "$RULER_X" "$((RULER_TOP + 6))"; sleep 1
"$OVM" mouse move 200 900; sleep 1
top_row=$(visible_row_y alpha.txt)
expect_true E-31-03 "clicking the top of the ruler returns to the first rows" "[[ '$top_row' =~ ^[0-9]+$ ]]"

shot=$(ruler_strip ruler-top)
marks=$(ruler_pixels "$shot" '#E5C07B')
expect_true E-31-04 "a modified file leaves an amber mark on the ruler" "[[ '$marks' =~ ^[0-9]+$ && '$marks' -ge 2 ]]"
guest "cd $ROOT_DIR/repo && git checkout -q -- tracked.txt"; sleep 5
shot=$(ruler_strip ruler-clean)
marks_after=$(ruler_pixels "$shot" '#E5C07B')
expect_true E-31-04 "and the mark goes away once the file is clean again" "[[ '$marks_after' =~ ^[0-9]+$ && '$marks_after' -lt '$marks' ]]"
guest "cd $ROOT_DIR/repo && printf 'changed\n' >> tracked.txt"

HALF_AMBER='#857357'
focus_tree
"$OVM" key end; sleep 1.5
shot=$(ruler_strip marks-away)
expect_true E-31-05 "marks for rows out of view are drawn at half strength" "[[ '$(ruler_pixels "$shot" "$HALF_AMBER")' -ge 2 && '$(ruler_pixels "$shot" '#E5C07B')' -eq 0 ]]"
"$OVM" mouse click "$RULER_X" "$((RULER_TOP + 6))"; sleep 1
"$OVM" mouse move 200 900; sleep 1
shot=$(ruler_strip marks-near)
expect_true E-31-05 "marks for rows in view are drawn in full" "[[ '$(ruler_pixels "$shot" '#E5C07B')' -ge 2 ]]"
ctl setScrollMarks false; sleep 2
expect E-31-05 "the setting turns the marks off" scrollMarks false
shot=$(ruler_strip marks-off)
expect_true E-31-05 "and no mark remains on the ruler" "[[ '$(ruler_pixels "$shot" '#E5C07B')' -eq 0 && '$(ruler_pixels "$shot" "$HALF_AMBER")' -eq 0 ]]"
expect_true E-31-05 "while the thumb stays" "[[ '$(ruler_pixels "$shot" '#607DBD')' -ge 40 ]]"
ctl setScrollMarks true; sleep 2
expect E-31-05 "and turns them back on" scrollMarks true

goto_root "$ROOT_DIR/empty"
shot=$(ruler_strip ruler-hidden)
thumb=$(ruler_pixels "$shot" '#607DBD')
expect_true E-31-03 "the ruler disappears when nothing overflows" "[[ '$thumb' =~ ^[0-9]+$ && '$thumb' -eq 0 ]]"
goto_root "$ROOT_DIR"

pending E-31-06 "extension trees" "needs a satellite pushed into the guest; ArtifactTree carries the same ruler and anchor by source"

summary
