#!/usr/bin/env bash
set -euo pipefail
[[ $(hostname) == omarchy-test ]] || { printf '%s\n' 'Run only in the assigned Omarchy guest' >&2; exit 1; }
[[ $# == 1 ]] || { printf '%s\n' 'usage: 96-delivery-adapter.sh ADAPTER' >&2; exit 1; }
adapter=$(realpath -e -- "$1")
[[ -x $adapter ]] || { printf 'adapter is not executable: %s\n' "$1" >&2; exit 1; }
work=$(mktemp -d /tmp/fileblade-delivery-96.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
real=$work/ovm
log=$work/argv
cat > "$real" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\0' "$@" > "$OVM_LOG"
exit "${OVM_EXIT:-0}"
EOF
chmod 755 "$real"
export OVM_REAL=$real OVM_LOG=$log

check_forward() {
  local expected_status=$1 status
  shift
  printf '%s\0' "$@" > "$work/expected"
  if OVM_EXIT=$expected_status "$adapter" "$@"; then status=0; else status=$?; fi
  [[ $status == "$expected_status" ]] || { printf 'unexpected exit for %s: %s\n' "$1" "$status" >&2; exit 1; }
  cmp -- "$work/expected" "$log"
}

ssh_args=(ssh "" '  ' $'line\nwith newline' "\"double\" 'single'" '$(printf forged); `printf forged` | & > < * ?')
check_forward 41 "${ssh_args[@]}"
printf 'PASS E-96-01 ssh preserves argv boundaries\n'

ipc_args=(ipc notifications "" '  ' $'line\nwith newline' "\"double\" 'single'" '$(printf forged); `printf forged` | & > < * ?')
check_forward 42 "${ipc_args[@]}"
printf 'PASS E-96-02 ordinary shell ipc preserves argv boundaries\n'

check_forward 17 status
check_forward 18 start --fresh
check_forward 19 stop
check_forward 20 shot 'name with spaces'
printf 'PASS E-96-03 status start stop shot forward exit codes\n'

mkdir "$work/tree"
unset SKIP_PUSH
: > "$log"
if "$adapter" push "$work/tree" > "$work/push-default" 2>&1; then push_status=0; else push_status=$?; fi
(( push_status != 0 )) || { printf '%s\n' 'push accepted with SKIP_PUSH unset' >&2; exit 1; }
grep -F 'push refused under SKIP_PUSH=1' "$work/push-default"
[[ ! -s $log ]] || { printf '%s\n' 'push forwarded with SKIP_PUSH unset' >&2; exit 1; }
printf 'PASS E-96-04 push remains adapter-owned with SKIP_PUSH unset (%s)\n' "$push_status"

: > "$log"
if SKIP_PUSH=1 "$adapter" push "$work/tree" > "$work/push-skipped" 2>&1; then
  push_status=0
else
  push_status=$?
fi
(( push_status != 0 )) || {
  printf '%s\n' 'push accepted with SKIP_PUSH=1' >&2
  exit 1
}
grep -F 'push refused under SKIP_PUSH=1' "$work/push-skipped"
[[ ! -s $log ]] || { printf '%s\n' 'SKIP_PUSH=1 push forwarded before refusal' >&2; exit 1; }
printf 'PASS E-96-05 SKIP_PUSH=1 refuses push before forwarding (%s)\n' "$push_status"

: > "$log"
if timeout --kill-after=1s 3s env OVM_REAL="$adapter" OVM_LOG="$log" "$adapter" status > "$work/self-recursion" 2>&1; then
  self_status=0
else
  self_status=$?
fi
(( self_status > 0 && self_status < 124 )) || { printf 'self-recursion was not refused: %s\n' "$self_status" >&2; exit 1; }
[[ ! -s $log ]] || { printf '%s\n' 'self-recursion reached the underlying ovm' >&2; exit 1; }
printf 'PASS E-96-06 self-recursive OVM_REAL refused\n'
