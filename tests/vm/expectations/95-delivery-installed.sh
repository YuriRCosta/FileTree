#!/usr/bin/env bash
set -euo pipefail
[[ $(hostname) == omarchy-test && $# == 2 ]] || { printf '%s\n' 'Run in the assigned guest: 95-delivery-installed.sh {install|check|update|rollback|remove} PAYLOAD' >&2; exit 1; }
action=$1
payload=$(realpath -e -- "$2")
native=${FILEBLADE_NATIVE_TOOL:-$payload/tools/native}
[[ -x $native ]]
[[ -x $payload/bin/fileblade && -z ${FILEBLADE_SPIKE_HOME:-} ]]
digest=$(sha256sum -- "$payload/payload.json"); digest=${digest%% *}
case $action in
  install)
    if "$native" status >/dev/null 2>&1; then printf '%s\n' 'Initial install requires an inactive test profile' >&2; exit 1; fi
    "$native" install "$payload"
    ;;
  check) ;;
  update)
    previous=$("$native" status | jq -er .payload)
    [[ $previous != "$digest" ]]
    "$native" install "$payload"
    ;;
  rollback)
    [[ $("$native" status | jq -er .previous) == "$digest" ]]
    "$native" rollback
    ;;
  remove)
    receipt=$("$native" status)
    [[ $(jq -r .payload <<< "$receipt") == "$digest" ]]
    installation=$(jq -r .installation <<< "$receipt")
    previous=$(jq -r '.previous // ""' <<< "$receipt")
    "$native" remove
    [[ ! -e $installation/versions/$digest && ! -L $installation/active && ! -L $HOME/.local/bin/fileblade ]]
    [[ -z $previous || ! -e $installation/versions/$previous ]]
    [[ $(cat "$installation/removed/receipt.json") == "$receipt" ]]
    printf 'PASS E-95-05 actual removal retains its receipt and withdraws runtime/launcher\n'
    exit
    ;;
  *) printf 'Unknown phase: %s\n' "$action" >&2; exit 1 ;;
esac
receipt=$("$native" status)
[[ $(jq -r .payload <<< "$receipt") == "$digest" ]]
installation=$(jq -r .installation <<< "$receipt")
runtime=$(readlink -f -- "$installation/active/runtime")
[[ $runtime == "$installation/versions/$digest" && $(readlink "$HOME/.local/bin/fileblade") == "$installation/launcher" ]]
"$native" verify "$runtime"
case $action in
  install) printf 'PASS E-95-01 actual initial install and stable launcher identity\n' ;;
  check) printf 'PASS E-95-02 active receipt and complete installed inventory\n' ;;
  update)
    [[ $(jq -r .previous <<< "$receipt") == "$previous" ]]
    printf 'PASS E-95-03 actual update records the previous payload\n'
    ;;
  rollback) printf 'PASS E-95-04 rollback restores the expected actual payload\n' ;;
esac
printf '%s\n' "$receipt"
