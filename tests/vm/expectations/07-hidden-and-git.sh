#!/usr/bin/env bash
# Hidden entries and Git presentation. Expectations E-07-01 .. E-07-08.
source "$(dirname "$0")/lib.sh"

require_guest

row_field() { "$OVM" ipc "$PLUGIN" tree 300 2>/dev/null | jq -r --arg n "$1" --arg f "$2" '[.entries[]?|select(.name==$n)][0][$f]'; }
repo_summary_text() {
  "$OVM" mouse move 500 400 >/dev/null
  ocr_crop ocr-repository-summary 378x30+0+135 400% 7 '0%,100%'
}

fixture >/dev/null
open_left
ctl setShowHidden true; sleep 2
goto_root "$ROOT_DIR"
expect_contains E-07-01 "hidden files are listed by default" "$(tree_names)" ".dotfile"

ctl setShowHidden false; sleep 3
expect_missing E-07-02 "turning hidden off removes dotfiles" "$(tree_names)" ".dotfile"
ctl setShowHidden true; sleep 3

goto_root "$ROOT_DIR/repo"
ctl refreshGit
wait_for "[[ \$(row_field tracked.txt gitStatus) != null && -n \$(row_field tracked.txt gitStatus) ]]" 25
expect_true E-07-04 "a gitignored entry is marked ignored" "[[ \$(row_field ignored.txt gitIgnored) == true ]]"
expect_true E-07-03 "a modified tracked file keeps its Git status" "[[ -n \$(row_field tracked.txt gitStatus) && \$(row_field tracked.txt gitStatus) != null ]]"
expect_true E-07-03 "a hidden tracked file still carries Git data" "[[ \$(row_field .gitignore gitRepoRoot) == '$ROOT_DIR/repo' ]]"

guest "printf 'hidden change\n' > '$ROOT_DIR/repo/.hidden-change.txt'"
ctl setShowHidden false; ctl refreshGit; sleep 3
wait_for "[[ \$(row_field .hidden-change.txt gitStatus) == '?' ]]" 25
expect_true E-07-03 "a changed hidden entry stays listed while hidden entries are off" "[[ \$(row_field .hidden-change.txt gitStatus) == '?' ]]"
expect_missing E-07-03 "an unchanged hidden entry stays out of the list" "$(tree_names)" ".gitignore"
guest "rm -f '$ROOT_DIR/repo/.hidden-change.txt'"
ctl setShowHidden true; ctl refreshGit; sleep 3

goto_root "$ROOT_DIR"
ctl refreshGit
wait_for "[[ \$(row_field repo gitBranch) != null && -n \$(row_field repo gitBranch) ]]" 25
expect_true E-07-05 "a repository root is marked as one" "[[ \$(row_field repo isGitRepo) == true ]]"
branch=$(row_field repo gitBranch)
expect_true E-07-05 "and its branch is available" "[[ '$branch' == master || '$branch' == main ]]"

if ! skip E-07-05; then
  saved_summary_fields=$(status | jq -r '.gitSummaryFields | join(",")')
  ctl setGitSummaryFields branch,worktree,ahead,behind,modified,added,untracked,deleted,renamed,copied,type_changed,conflicted
  ctl setPriorityColumns size; sleep 2
  expect_missing E-07-05 "a merely listed repository keeps its chosen column" "$(tree_text)" "M1"
  goto_root "$ROOT_DIR/repo"
  ctl refreshGit
  wait_for "[[ \$(row_field repo gitSummary) == *'\"ok\":true'* ]]" 25
  expect_true E-07-05 "the opened repository reports its modified-file count" "[[ \$(row_field repo gitSummary | jq -r .modified) == 1 ]]"
  expect_contains E-07-05 "its root row displays the Git summary" "$(repo_summary_text)" "M1"
  ctl setPriorityColumns modified; sleep 2
  expect_contains E-07-05 "changing columns keeps the opened repository summary" "$(repo_summary_text)" "M1"
  expect_contains E-07-05 "the branch appears beside its counts" "$(repo_summary_text)" "$branch"
  guest "cd '$ROOT_DIR/repo' && printf added > summary-added.txt && printf untracked > summary-untracked.txt && git add -- summary-added.txt"
  ctl refreshGit
  wait_for "[[ \$(row_field repo gitSummary | jq -r .added) == 1 ]]" 25
  expect_true E-07-05 "added and untracked counts are separate" "[[ \$(row_field repo gitSummary | jq -r '.added == 1 and .untracked == 1') == true ]]"
  expect_true E-07-05 "staged additions never enter the untracked count" "[[ \$(row_field summary-added.txt gitUntrackedCount) == 0 && \$(row_field summary-untracked.txt gitUntrackedCount) == 1 ]]"
  ctl setGitSummaryFields modified,added,untracked; sleep 2
  expect_missing E-07-05 "the branch can be hidden" "$(repo_summary_text)" "$branch"
  ctl setGitSummaryFields ""; sleep 2
  expect_missing E-07-05 "hiding every field restores the normal columns" "$(repo_summary_text)" "M1"
  restart_shell
  expect_true E-07-05 "summary choices survive a restart" "[[ \$(status | jq -c .gitSummaryFields) == '[]' ]]"
  guest "cd '$ROOT_DIR/repo' && git rm -q --cached -- summary-added.txt && rm -f -- summary-added.txt summary-untracked.txt"
  ctl setGitSummaryFields "$saved_summary_fields"
  ctl refreshGit
  goto_root "$ROOT_DIR"
fi
if [[ ${ONLY:-} == E-07-05 ]]; then summary; fi

expect E-07-06 "git status is on by default" gitEnabled true
ctl setGitStatusDetails false; sleep 2
ctl setGitStatusDetails true; sleep 2
expect E-07-06 "and can be turned back on" gitEnabled true

ctl setPriorityColumns updated; sleep 2
before_cols=$(field priorityColumns)
ctl setPriorityColumns size; sleep 2
expect_true E-07-07 "the priority columns change" "[[ \$(field priorityColumns) != '$before_cols' ]]"
ctl setPriorityColumns updated; sleep 2


goto_root "$ROOT_DIR/repo"
ctl refreshGit
wait_for "[[ \$(row_field tracked.txt gitStatus) != null && -n \$(row_field tracked.txt gitStatus) ]]" 25
guest "printf 'fresh\\n' > $ROOT_DIR/repo/fresh.txt"
wait_for "[[ \$(row_field fresh.txt gitStatus) != null && -n \$(row_field fresh.txt gitStatus) ]]" 25
expect_true E-07-08 "a new file gets a Git status without a manual refresh" "[[ -n \$(row_field fresh.txt gitStatus) && \$(row_field fresh.txt gitStatus) != null ]]"
guest "rm -f $ROOT_DIR/repo/fresh.txt"; sleep 2
goto_root "$ROOT_DIR"

summary
