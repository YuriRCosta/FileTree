#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

require_guest

layout_path=$(field bladeLayoutPath) || exit 1
[[ $layout_path == /*/blades.json ]] || { fail harness "read the blade layout path" "got [$layout_path]"; summary; }
original_layout=$(guest "cat -- $(printf '%q' "$layout_path")") || exit 1
if ! jq -e '.blades.left.slots and .blades.right.slots' <<<"$original_layout" >/dev/null; then
  fail harness "save the original layout" "layout document is unavailable"
  summary
fi
original_state=$(field welcomeState) || exit 1
original_focus=$(field focusedBlade) || exit 1
network_dir=""
network_guard_pid=""
network_v4_defaults=""
network_v6_defaults=""

network_routes_match() {
  local current_v4 current_v6
  current_v4=$(guest 'ip -4 route show table main default' | tr -d '\r') || return 1
  current_v6=$(guest 'ip -6 route show table main default' | tr -d '\r') || return 1
  [[ $current_v4 == "$network_v4_defaults" && $current_v6 == "$network_v6_defaults" ]]
}

restore_network() {
  [[ -n $network_dir ]] || return 0
  "$OVM" sudo "ip -4 route restore < '$network_dir/v4' >/dev/null 2>&1 || true
    ip -6 route restore < '$network_dir/v6' >/dev/null 2>&1 || true" >/dev/null 2>&1 || return 1
  network_routes_match || return 1
  [[ -z $network_guard_pid ]] || "$OVM" sudo "kill -- -$network_guard_pid >/dev/null 2>&1 || true" >/dev/null 2>&1 || return 1
  "$OVM" sudo "rm -rf -- '$network_dir'" >/dev/null 2>&1 || return 1
  network_guard_pid=""
  network_dir=""
}

restore_layout() {
  local failed=0 edge mode open
  ctl setWelcomeState "$original_state" || failed=1
  for edge in left right; do
    ctl setBladeSlots "$edge" "base64:$(jq -c ".blades.$edge.slots" <<<"$original_layout" | base64 -w0)" || failed=1
    ctl setBladeWidth "$edge" "$(jq -r ".blades.$edge.width" <<<"$original_layout")" || failed=1
    mode=$(jq -r ".blades.$edge.mode" <<<"$original_layout")
    if [[ $mode == window ]]; then ctl undockBlade "$edge"; else ctl dockBlade "$edge"; fi || failed=1
    open=$(jq -r ".blades.$edge.open" <<<"$original_layout")
    if [[ $open == true ]]; then ctl openBlade "$edge"; else ctl closeBlade "$edge"; fi || failed=1
  done
  if [[ -n $original_focus ]]; then ctl focusBlade "$original_focus"; else ctl releaseBladeFocus; fi || failed=1
  return "$failed"
}

cleanup() {
  local status=$?
  restore_network || status=1
  restore_layout || status=1
  return "$status"
}
trap 'cleanup || exit 1' EXIT
trap 'exit 130' INT TERM

right_modules() { "$OVM" ipc "$PLUGIN" blades | jq -c '[.blades.right.slots[].modules[].module]'; }
welcome_state() { field welcomeState; }

network_dir=$(guest 'mktemp -d /tmp/fileblade-e29-network.XXXXXX' | tr -d '\r') || exit 1
[[ $network_dir =~ ^/tmp/fileblade-e29-network\.[[:alnum:]]+$ ]] || { fail harness "create the network snapshot" "unexpected path [$network_dir]"; summary; }
network_v4_defaults=$(guest 'ip -4 route show table main default' | tr -d '\r') || exit 1
network_v6_defaults=$(guest 'ip -6 route show table main default' | tr -d '\r') || exit 1
if [[ -z $network_v4_defaults && -z $network_v6_defaults ]]; then
  fail harness "capture the guest WAN routes" "the guest has no default route to isolate"
  summary
fi
if ! guest 'ip -4 route get 1.1.1.1 >/dev/null 2>&1 || ip -6 route get 2606:4700:4700::1111 >/dev/null 2>&1'; then
  fail harness "capture the guest WAN routes" "the guest has no external route before isolation"
  summary
fi
if ! "$OVM" sudo "ip -4 route save table main default > '$network_dir/v4' &&
  ip -6 route save table main default > '$network_dir/v6'" >/dev/null 2>&1; then
  fail harness "save the guest network" "ip route save failed"
  summary
fi
"$OVM" sudo "setsid bash -c $(printf '%q' "printf '%s' \"\$\$\" > '$network_dir/guard.pid'
  sleep 180
  ip -4 route restore < '$network_dir/v4' >/dev/null 2>&1 || true
  ip -6 route restore < '$network_dir/v6' >/dev/null 2>&1 || true
  rm -rf -- '$network_dir'") </dev/null >/dev/null 2>&1 &" || exit 1
wait_for "guest 'test -s $network_dir/guard.pid'" 5 || exit 1
network_guard_pid=$(guest "cat '$network_dir/guard.pid'" | tr -d '\r') || exit 1
[[ $network_guard_pid =~ ^[0-9]+$ ]] || { fail harness "schedule network restoration" "unexpected guard pid [$network_guard_pid]"; summary; }
"$OVM" sudo "kill -0 $network_guard_pid" || exit 1
if ! "$OVM" sudo 'while ip -4 route del default table main 2>/dev/null; do :; done
  while ip -6 route del default table main 2>/dev/null; do :; done' >/dev/null 2>&1; then
  fail harness "disable the guest WAN routes" "ip route del failed"
  summary
fi
network_offline() {
  if guest 'ip -4 route get 1.1.1.1 >/dev/null 2>&1 || ip -6 route get 2606:4700:4700::1111 >/dev/null 2>&1'; then
    "$OVM" sudo 'while ip -4 route del default table main 2>/dev/null; do :; done
      while ip -6 route del default table main 2>/dev/null; do :; done' >/dev/null 2>&1 || true
    return 1
  fi
  return 0
}
if ! wait_for network_offline 8; then
  fail harness "disable the guest WAN routes" "the guest still has an external route"
  summary
fi
expect_true E-29-05 "SSH survives guest WAN isolation" 'guest_alive && guest true'
expect_true E-29-05 "external TCP connectivity is unavailable" '! guest "timeout 5 bash -c '\''exec 3<>/dev/tcp/1.1.1.1/443'\''"'

ctl setWelcomeState ""
ctl setBladeSlots right "base64:$(printf '%s' '[{"id":"welcome","modules":[{"module":"welcome"},{"module":"notes"}],"active":0}]' | base64 -w0)"
ctl openBlade right
ctl focusBlade right
sleep 1
expect_true E-29-01 "Welcome and Notes are available together" '[[ $(right_modules) == '\''["welcome","notes"]'\'' ]]'
expect_contains E-29-01 "Welcome introduces built-in blades" "$(screen_text)" "Welcome to FileBlade"
expect_contains E-29-05 "help is visible without an external route" "$(screen_text)" "Find your way"
expect_true E-29-05 "the guest still has no external route while help is displayed" '! guest '\''ip -4 route get 1.1.1.1 >/dev/null 2>&1 || ip -6 route get 2606:4700:4700::1111 >/dev/null 2>&1'\'''
"$OVM" shot E-29-05-offline-help
if restore_network; then
  pass E-29-05 "the original guest routes are restored"
else
  fail E-29-05 "restore the guest network" "saved routes do not match; the failsafe remains armed"
  summary
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
pending E-29-05 "optional catalog rejection" "the sealed Welcome payload has no external catalog transport; malformed, oversized and newer-schema inputs are not exercised here"
summary
