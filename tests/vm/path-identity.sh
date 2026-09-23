#!/usr/bin/env bash
source "$(dirname "$0")/expectations/lib.sh"
require_guest
dock_blades
reset_modules
ctl setSidebarWidth 380
ctl setPriorityColumns type
case_root=$(guest 'mktemp -d /home/omarchy/filetree-byte-path.XXXXXX')
[[ $case_root == /home/omarchy/filetree-byte-path.* ]] || exit 2
raw_uri="file://$case_root/%FF.txt"
unicode_path="$case_root/�.txt"
guest "printf 'raw bytes' > '$case_root/'\$'\\377.txt'
  printf 'unicode spelling' > '$unicode_path'
  printf 'literal escape' > '$case_root/\\xFF.txt'
  mkdir '$case_root/dest'"
open_left
goto_root "$case_root"
ctl focusTree
sleep 2

select_byte_row() {
  local label=$1 expected=$2 index step command='wtype -k Home'
  index=$(row_index "$label")
  if [[ ! $index =~ ^[0-9]+$ ]]; then
    fail path-identity "select $label" "row is absent"
    summary
  fi
  ctl focusTree
  for ((step=0; step<index; step++)); do command+=' -k Down'; done
  guest "$command"
  sleep 1
  if [[ $(field selectedPath) != "$expected" ]]; then
    fail path-identity "select $label" "selectedPath=$(field selectedPath)"
    summary
  fi
}

expect_contains path-identity "raw byte has an escaped display name" "$(tree_names)" '\xFF.txt'
expect_contains path-identity "Unicode spelling has its own display name" "$(tree_names)" '�.txt'
expect_contains path-identity "literal escape is distinguishable" "$(tree_names)" '\\xFF.txt'
paths=$("$OVM" ipc "$PLUGIN" tree 20 | jq -r '[.entries[] | .path] | unique | length')
expect_true path-identity "all five tree identities are distinct" "[[ $paths == 5 ]]"
"$OVM" shot path-identity-listing >/dev/null

select_byte_row '\xFF.txt' "$raw_uri"
"$OVM" key f2
sleep 2
expect path-identity "F2 opens the rename dialog for the byte name" actionMenuMode rename
expect path-identity "the dialog keeps the byte-faithful target" actionMenuPath "$raw_uri"
"$OVM" shot path-identity-rename >/dev/null
"$OVM" key ret
sleep 2
expect_out path-identity "unchanged rename preserves the byte name" "test -f '$case_root/'\$'\\377.txt' && echo yes" yes

"$OVM" key r
sleep 2
"$OVM" type renamed.txt
"$OVM" key ret
sleep 3
expect_out path-identity "rename changes the selected file" "cat '$case_root/renamed.txt'" 'raw bytes'
expect_out path-identity "rename leaves the Unicode neighbour unchanged" "cat '$unicode_path'" 'unicode spelling'
"$OVM" key ctrl-z
sleep 3
expect_out path-identity "undo restores the original bytes" "cat '$case_root/'\$'\\377.txt'" 'raw bytes'

for operation in copy move; do
  select_byte_row '\xFF.txt' "$raw_uri"
  if [[ $operation == copy ]]; then "$OVM" key y; else "$OVM" key x; fi
  sleep 1
  select_byte_row dest "$case_root/dest"
  "$OVM" key p
  sleep 3
  expect_out path-identity "$operation preserves the destination's filename bytes" "cat '$case_root/dest/'\$'\\377.txt'" 'raw bytes'
  expect_out path-identity "$operation leaves the Unicode neighbour unchanged" "cat '$unicode_path'" 'unicode spelling'
  "$OVM" key ctrl-z
  sleep 3
  expect_out path-identity "undo $operation restores the source state" "test -f '$case_root/'\$'\\377.txt' && test ! -e '$case_root/dest/'\$'\\377.txt' && echo yes" yes
done

select_byte_row '\xFF.txt' "$raw_uri"
"$OVM" key delete
sleep 2
"$OVM" key tab
sleep 0.5
"$OVM" key ret
sleep 3
expect_out path-identity "Trash removes only the byte spelling" "test ! -e '$case_root/'\$'\\377.txt' && cat '$unicode_path'" 'unicode spelling'
"$OVM" key ctrl-z
sleep 3
expect_out path-identity "undo Trash restores the original bytes" "cat '$case_root/'\$'\\377.txt'" 'raw bytes'
expect_out path-identity "all actions leave the literal escape file unchanged" "cat '$case_root/\\xFF.txt'" 'literal escape'
"$OVM" shot path-identity-restored >/dev/null
printf 'Fixture retained: %s\n' "$case_root"
summary
