#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

original_blades=$("$OVM" ipc "$PLUGIN" blades) || exit 1
original_state=$(field welcomeState) || exit 1
cleanup() {
  ctl setWelcomeState "$original_state"
  for edge in left right; do
    ctl setBladeSlots "$edge" "base64:$(jq -c ".blades.$edge.slots" <<<"$original_blades" | base64 -w0)"
    if [[ $(jq -r ".blades.$edge.open" <<<"$original_blades") == true ]]; then
      ctl openBlade "$edge"
    else
      ctl closeBlade "$edge"
    fi
  done
}
trap cleanup EXIT

right_modules() { "$OVM" ipc "$PLUGIN" blades | jq -c '[.blades.right.slots[].modules[].module]'; }
welcome_state() { field welcomeState; }

ctl setWelcomeState ""
ctl setBladeSlots right "base64:$(printf '%s' '[{"id":"welcome","modules":[{"module":"welcome"},{"module":"notes"}],"active":0}]' | base64 -w0)"
ctl openBlade right
ctl focusBlade right
sleep 1
expect_true E-29-01 "Welcome and Notes are available together" '[[ $(right_modules) == '\''["welcome","notes"]'\'' ]]'
expect_contains E-29-01 "Welcome introduces built-in blades" "$(screen_text)" "Welcome to FileBlade"
if [[ $(guest 'if ip -4 route get 1.1.1.1 >/dev/null 2>&1 || ip -6 route get 2606:4700:4700::1111 >/dev/null 2>&1; then echo online; else echo offline; fi') == offline ]]; then
  expect_contains E-29-05 "help is visible without an external route" "$(screen_text)" "Find your way"
else
  pending E-29-05 "offline help" "Run with the guest external routes disabled; catalog rejection is covered by the Welcome QML suites."
fi
expect_missing E-29-02 "Welcome has no install offer" "$(screen_text)" "Install agent extensions"
expect_true E-29-02 "all four agent blades are core modules" 'status | jq -e '\''.bladeModules | contains(["skills","memory","hooks","mcp"])'\'''
expect_true E-29-02 "legacy install IPC reports built-in" '[[ $("$OVM" ipc "$PLUGIN.control" welcomeInstall) == built-in ]]'
expect E-29-02 "no installer is running" welcomeInstalling false

ctl welcomeDismiss
sleep 1
expect_true E-29-03 "dismissal retains Notes" '[[ $(right_modules) == '\''["notes"]'\'' ]]'
expect_true E-29-03 "dismissal is recorded" '[[ $(welcome_state) == dismissed ]]'
ctl addBladeModule right welcome
ctl focusBlade right
sleep 1
expect_contains E-29-04 "a dismissed Welcome can be reopened" "$(right_modules)" welcome
expect_true E-29-04 "reopening preserves dismissal" '[[ $(welcome_state) == dismissed ]]'
expect_contains E-29-04 "reopened Welcome renders" "$(screen_text)" "Welcome to FileBlade"
"$OVM" shot E-29-reopened

ctl setWelcomeState installed
sleep 1
expect_contains E-29-04 "legacy installed state also permits reopen" "$(right_modules)" welcome
expect_true E-29-04 "legacy state is preserved" '[[ $(welcome_state) == installed ]]'
summary
