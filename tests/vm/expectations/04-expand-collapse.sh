#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

expanded_of() { "$OVM" ipc "$PLUGIN" tree 300 2>/dev/null | jq -r --arg n "$1" '[.entries[]?|select(.name==$n)][0].expanded'; }
press_on() { click_row "$1"; "$OVM" key "$2"; sleep 2.5; }
fold() { "$OVM" key z; "$OVM" key "$1"; sleep 2.5; }
fold_on() { click_row "$1"; fold "$2"; }

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"
focus_tree

fold_on deep o
expect_true E-04-01 "zo expands in place" "[[ \$(expanded_of deep) == true ]]"
fold o
expect E-04-01 "zo on an open folder keeps the cursor" selectedPath "$ROOT_DIR/deep"
expect E-04-01 "and does not enter it" rootPath "$ROOT_DIR"
fold c
expect_true E-04-02 "zc collapses" "[[ \$(expanded_of deep) == false ]]"
fold c
expect E-04-02 "zc on a closed folder keeps the cursor" selectedPath "$ROOT_DIR/deep"
"$OVM" key z; "$OVM" key esc; sleep 1
expect E-04-02 "Escape cancels the fold prefix without closing the blade" open true

fold_on empty o
expect_true E-04-03 "an empty directory expands" "[[ \$(expanded_of empty) == true ]]"
empty_error=$("$OVM" ipc "$PLUGIN" tree 300 2>/dev/null | jq -r '[.entries[]?|select(.name=="empty")][0].error')
expect_true E-04-03 "and reports no error" "[[ -z '$empty_error' || '$empty_error' == null ]]"
expect_missing E-04-03 "and shows no children" "$(tree_names)" "late.txt"
fold_on empty c

before_root=$(field rootPath)
press_on deep ret
expect_true E-04-04 "Enter expands rather than changing the root" "[[ \$(field rootPath) == '$before_root' ]]"
expect_true E-04-04 "and the directory is open" "[[ \$(expanded_of deep) == true ]]"

ctl refresh; sleep 4
expect_true E-04-05 "expansion survives a refresh" "[[ \$(expanded_of deep) == true ]]"

ctl collapsePath "$ROOT_DIR/deep"; sleep 2
guest "printf 'later\n' > $ROOT_DIR/deep/added-while-closed.txt"; sleep 2
ctl expandPath "$ROOT_DIR/deep"; sleep 3
expect_contains E-04-06 "reopening shows the current contents" "$(tree_names)" "added-while-closed.txt"

guest "mkdir -p $ROOT_DIR/dest/other; ln -sfn .. $ROOT_DIR/deep/cycle"
ctl refresh; sleep 3
ctl expandPath "$ROOT_DIR/dest"; sleep 2
press_on deep shift-right
wait_for '[[ $(expanded_of inner) == true ]]'
expect_true E-04-01 "Shift+Right expands the selected folder and its descendants" "[[ \$(expanded_of deep) == true && \$(expanded_of inner) == true ]]"
expect_contains E-04-01 "and loads the nested contents" "$(tree_names)" "deep.txt"
expect E-04-01 "and keeps the cursor on the selected folder" selectedPath "$ROOT_DIR/deep"
expect E-04-01 "and leaves the tree root unchanged" rootPath "$ROOT_DIR"
expect_true E-04-01 "and leaves an unrelated branch alone" "[[ \$(expanded_of dest) == true && \$(expanded_of other) == false ]]"
expect_true E-04-01 "and does not follow a descendant directory symlink" "[[ \$(expanded_of cycle) == false ]]"
"$OVM" shot selected-subtree-expanded >/dev/null

"$OVM" key shift-left; sleep 2
expect_true E-04-02 "Shift+Left collapses the selected branch only" "[[ \$(expanded_of deep) == false && \$(expanded_of dest) == true ]]"
expect_missing E-04-02 "and removes its nested rows" "$(tree_names)" "deep.txt"
expect E-04-02 "and keeps the selected folder in place" selectedPath "$ROOT_DIR/deep"
fold o
expect_true E-04-02 "reopening does not restore descendants' expansion" "[[ \$(expanded_of deep) == true && \$(expanded_of inner) == false ]]"
"$OVM" shot selected-subtree-reopened >/dev/null

click_row alpha.txt
before_names=$(tree_names)
fold o
fold c
"$OVM" key shift-right
"$OVM" key shift-left; sleep 2
expect_true E-04-01 "fold keys on a file leave the tree unchanged" "[[ \$(tree_names) == '$before_names' ]]"
expect_out E-04-01 "and do not open its application" "hyprctl -j clients | jq length" 0
fold_on deep shift-o
expect_true E-04-01 "zO recursively opens the selected branch" "[[ \$(expanded_of inner) == true && \$(expanded_of other) == false ]]"
fold shift-c
expect_true E-04-02 "zC closes the selected branch without changing siblings" "[[ \$(expanded_of deep) == false && \$(expanded_of dest) == true ]]"
click_row alpha.txt
fold shift-r
expect_true E-04-01 "zR from a file recursively expands both branches" "[[ \$(expanded_of inner) == true && \$(expanded_of other) == true ]]"
expect E-04-01 "and retains the selected file" selectedPath "$ROOT_DIR/alpha.txt"
fold shift-m
expect_true E-04-02 "zM from a file collapses the entire tree" "[[ \$(row_index deep) == null ]]"
expect E-04-02 "and selects the remaining root row" selectedPath "$ROOT_DIR"
fold o
expect_true E-04-02 "reopening the root leaves its child folders collapsed" "[[ \$(expanded_of deep) == false && \$(expanded_of dest) == false ]]"
guest "rm -f $ROOT_DIR/deep/cycle; rmdir $ROOT_DIR/dest/other"

summary
