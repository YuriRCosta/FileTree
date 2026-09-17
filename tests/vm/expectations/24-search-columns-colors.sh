#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

deep_search() {
  ctl setSearchDeep true
  ctl search "$1"
  sleep 2
  wait_for "[[ \$(field searchBusy) == false ]]" 30
}

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
ctl clearSearch; ctl setSearchOptions false false; sleep 2

deep_search "alpha"
with_alpha=$(search_names)
deep_search "-alpha"
without_alpha=$(search_names)
expect_contains E-24-01 "a bare word matches" "$with_alpha" "alpha.txt"
expect_missing E-24-01 "a leading minus excludes it" "$without_alpha" "alpha.txt"
deep_search "^alpha"
expect_contains E-24-01 "a caret anchors the start" "$(search_names)" "alpha.txt"
deep_search "txt\$"
expect_contains E-24-01 "a dollar anchors the end" "$(search_names)" "alpha.txt"

deep_search "type:dir"
dirs_only=$(search_names)
expect_contains E-24-02 "type:dir keeps directories" "$dirs_only" "deep"
expect_missing E-24-02 "and drops files" "$dirs_only" "alpha.txt"
deep_search "format:png"
expect_contains E-24-02 "format:png keeps images" "$(search_names)" "small.png"

deep_search "content:bravo"
expect_contains E-24-03 "content: finds matching lines" "$(search_names)" "bravo.txt"

deep_search "alpha"
ctl setSearchLayout false
focus_tree
"$OVM" key ctrl-shift-b; sleep 3
tree_layout=$(search_names)
expect E-24-04 "Ctrl+Shift+B switches to tree results" searchTreeLayout true
expect_true E-24-04 "the layout switches without changing the search" "[[ \$(field searchQuery) == alpha ]]"
expect_true E-24-04 "and results are still present" "[[ -n '$tree_layout' ]]"
"$OVM" key ctrl-b; sleep 1
expect E-24-04 "Ctrl+B pages without changing the layout" searchTreeLayout true
"$OVM" key ctrl-shift-b; sleep 2
expect E-24-04 "Ctrl+Shift+B switches back to a flat list" searchTreeLayout false
ctl clearSearch; ctl setSearchDeep false; sleep 2

before_cols=$(field priorityColumns)
ctl setPriorityColumns "size,updated"; sleep 3
expect_contains E-24-05 "a column can be added" "$(field priorityColumns)" "size"
ctl setPriorityColumns "updated"; sleep 3
expect_missing E-24-05 "and removed again" "$(field priorityColumns)" "size"

ctl setPriorityColumns "size,updated"; sleep 2
ctl close; sleep 3; ctl open; sleep 4
expect_contains E-24-09 "columns survive close and reopen" "$(field priorityColumns)" "size"
ctl setPriorityColumns "$before_cols"; sleep 2

before_property=$(field priorityProperty)
ctl setPriorityProperty size; sleep 3
expect_true E-24-07 "a column can become the sort key" "[[ \$(field priorityProperty) == size ]]"
ctl cyclePriorityProperty; sleep 3
expect_true E-24-07 "and cycling changes it again" "[[ \$(field priorityProperty) != size ]]"
ctl setPriorityProperty "${before_property:-modified}" >/dev/null 2>&1; sleep 2

pending E-24-06 "dragging a column moves it without obscuring the labels" "column drag needs a frame by frame capture"
pending E-24-08 "a column filter leaves a visible marker until cleared" "column filters are not reported in the status document"

details() { status | jq -c '.gitStatusDetails'; }
ctl setGitStatusDetails modified,new,deleted; sleep 3
shown=$(details)
ctl setGitStatusDetails modified; sleep 3
hidden=$(details)
expect_true E-24-10 "the git details on show can be changed" "[[ '$shown' != '$hidden' ]]"
ctl setGitStatusDetails modified,new,deleted; sleep 3
expect_true E-24-10 "and restored" "[[ \$(details) == '$shown' ]]"

ctl setFolderColor "$ROOT_DIR/deep" "#7aa2f7"; sleep 3
picked=$("$OVM" ipc "$PLUGIN" folderColor "$ROOT_DIR/deep" 2>/dev/null | jq -r '.color')
expect_true E-24-11 "a palette colour applies at once" "[[ '$picked' == '#7aa2f7' ]]"
ctl setFolderColor "$ROOT_DIR/deep" "#123456"; sleep 3
custom=$("$OVM" ipc "$PLUGIN" folderColor "$ROOT_DIR/deep" 2>/dev/null | jq -r '.color')
expect_true E-24-11 "and a six digit custom colour applies too" "[[ '$custom' == '#123456' ]]"

STATE=/home/omarchy/.local/state/omarchy/fileblade/state.json
scope_now() { guest "cat $STATE 2>/dev/null" | jq -r '..|objects|.folderColorScope? // empty' 2>/dev/null | head -1; }
before_scope=$(scope_now)
ctl setFolderColorScope row; sleep 3
expect_true E-24-12 "the colour scope changes" "[[ \$(scope_now) == row ]]"
ctl setFolderColorScope "${before_scope:-icon}"; sleep 2
ctl clearFolderColor "$ROOT_DIR/deep"; sleep 3
cleared=$("$OVM" ipc "$PLUGIN" folderColor "$ROOT_DIR/deep" 2>/dev/null | jq -r '.color')
expect_true E-24-12 "reset returns to the theme colour" "[[ -z '$cleared' || '$cleared' == null || '$cleared' == '' ]]"

pending E-24-13 "the column menu has a heading and a close glyph" "popup contents are not reported over IPC"

summary
