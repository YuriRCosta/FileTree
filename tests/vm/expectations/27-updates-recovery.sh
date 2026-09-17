#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

GUEST_PLUGIN=/home/omarchy/.config/omarchy/plugins/$PLUGIN

fixture >/dev/null
open_left
goto_root "$ROOT_DIR"

pending E-27-01 "an update chip appears when an update is available" "needs a checkout the guest can see as behind its remote"
pending E-27-02 "the chip explains what can update and never rewrites running files" "depends on E-27-01"
pending E-27-03 "a checkout with local work reports the update as skipped" "depends on E-27-01"

guest "cp $GUEST_PLUGIN/manifest.json /tmp/manifest.backup.json"
"$(dirname "$0")/../stop-shell" || exit 1
guest "sed -i 's/\"version\": *\"[^\"]*\"/\"version\": \"9.9.9\"/' $GUEST_PLUGIN/manifest.json"
restart_shell
open_left; goto_root "$ROOT_DIR"
skew=$(summary_text)
expect_contains E-27-04 "a mismatched backend says so" "$skew" "Backend"
expect_contains E-27-04 "and says how to fix it" "$skew" "update"

"$(dirname "$0")/../stop-shell" || exit 1
guest "cp /tmp/manifest.backup.json $GUEST_PLUGIN/manifest.json"
restart_shell
open_left; goto_root "$ROOT_DIR"
restored=$(summary_text)
expect_missing E-27-04 "and the warning clears once they agree" "$restored" "does not match plugin"

expect_true E-27-05 "an offline update check leaves no busy indicator" "[[ \$(field operationBusy) == false ]]"
expect_true E-27-05 "and the tree is usable" "[[ \$(field treeEntries) -ge 1 ]]"
expect_missing E-27-05 "with no repeated prompt on screen" "$(summary_text)" "Checking for updates"


pending E-27-06 "an up to date check shows a notice that clears after ten seconds" "the guest is offline, so no check ever succeeds; UpdateController.qml holds the notice for upToDateNoticeMs and clears it on a timer"

summary
