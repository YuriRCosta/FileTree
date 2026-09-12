#!/usr/bin/env bash
set -euo pipefail
[[ $(hostname) == omarchy-test ]] || { printf '%s\n' 'Run only in the assigned Omarchy guest' >&2; exit 1; }
[[ $# == 1 ]] || { printf '%s\n' 'usage: 91-delivery-install.sh PAYLOAD' >&2; exit 1; }
payload=$(realpath -e -- "$1")
native=$payload/tools/native
work=$(mktemp -d /tmp/fileblade-delivery-91.XXXXXX)
worker=
trap '[[ -z $worker ]] || kill -KILL -- -"$worker" 2>/dev/null || true; rm -rf -- "$work"' EXIT
export HOME=$work/home XDG_DATA_HOME=$work/data XDG_CONFIG_HOME=$work/config XDG_STATE_HOME=$work/state
mkdir -p "$HOME/.local/bin" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME"
printf 'keep settings\n' > "$XDG_CONFIG_HOME/settings"
printf 'keep notes\n' > "$XDG_STATE_HOME/notes"
printf 'unrelated\n' > "$HOME/.local/bin/fileblade"
if "$native" install "$payload"; then exit 1; fi
[[ $(cat "$HOME/.local/bin/fileblade") == unrelated ]]
rm -- "$HOME/.local/bin/fileblade"
printf 'PASS E-91-01 unrelated launcher preserved\n'
"$native" install "$payload"
installation=$XDG_DATA_HOME/fileblade/installation
first=$(jq -r .payload "$installation/active/receipt.json")
[[ -L $HOME/.local/bin/fileblade && $(readlink -f "$installation/active/runtime") == "$installation/versions/$first" ]]
"$native" install "$payload"
[[ $(jq -r .payload "$installation/active/receipt.json") == "$first" ]]
printf 'PASS E-91-02 first and repeated install\n'
cp -a -- "$payload" "$work/next"
printf '\n' >> "$work/next/payload.json"
"$native" install "$work/next"
second=$(jq -r .payload "$installation/active/receipt.json")
[[ $first != "$second" && $(jq -r .previous "$installation/active/receipt.json") == "$first" ]]
"$native" rollback
[[ $(jq -r .payload "$installation/active/receipt.json") == "$first" ]]
printf 'PASS E-91-03 activation and rollback\n'
flock -s "$installation/lock" sleep 30 &
holder=$!
sleep 0.1
if "$native" install "$work/next"; then kill "$holder"; exit 1; fi
kill "$holder"
wait "$holder" || true
[[ $(jq -r .payload "$installation/active/receipt.json") == "$first" ]]
printf 'PASS E-91-04 live session lock prevents activation\n'
mkdir "$work/path"
cat > "$work/path/fault" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
operation=${0##*/}
if [[ $operation == cp && $CINDER_FAULT == copy && ${*: -1} == *'/.payload.'* ]]; then
  /usr/bin/cp -- "$1/Service.qml" "${*: -1}/Service.qml" 2>/dev/null || true
  touch "$CINDER_MARKER"
  sleep 60
elif [[ $operation == mv && $CINDER_FAULT == activation-* && ${*: -1} == */installation/active ]]; then
  if [[ $CINDER_FAULT == activation-after ]]; then /usr/bin/mv "$@"; fi
  touch "$CINDER_MARKER"
  sleep 60
fi
exec "/usr/bin/$operation" "$@"
EOF
chmod 755 "$work/path/fault"
ln -s fault "$work/path/cp"
ln -s fault "$work/path/mv"
for phase in copy activation-before activation-after; do
  cp -a -- "$payload" "$work/$phase"
  printf '\n%s\n' ' ' >> "$work/$phase/payload.json"
  printf '%*s' "${#phase}" '' >> "$work/$phase/payload.json"
  export CINDER_FAULT=$phase CINDER_MARKER=$work/$phase-reached
  setsid env PATH="$work/path:$PATH" "$native" install "$work/$phase" > "$work/$phase.log" 2>&1 &
  worker=$!
  for attempt in {1..200}; do
    [[ ! -f $CINDER_MARKER ]] || break
    kill -0 "$worker" 2>/dev/null || { cat "$work/$phase.log"; exit 1; }
    sleep 0.05
  done
  [[ -f $CINDER_MARKER ]] || { cat "$work/$phase.log"; exit 1; }
  kill -KILL -- -"$worker"
  wait "$worker" || true
  worker=
  current=$(jq -r .payload "$installation/active/receipt.json")
  if [[ $phase != activation-after ]]; then [[ $current == "$first" ]]; fi
  [[ $(readlink -f "$installation/active/runtime") == "$installation/versions/$current" ]]
  "$native" verify "$installation/active/runtime"
  "$native" install "$work/$phase"
  "$native" rollback
  [[ $(jq -r .payload "$installation/active/receipt.json") == "$first" ]]
  printf 'PASS E-91-05 interrupted %s and retry\n' "$phase"
done
[[ $(cat "$XDG_CONFIG_HOME/settings") == 'keep settings' && $(cat "$XDG_STATE_HOME/notes") == 'keep notes' ]]
printf 'PASS E-91-06 settings and Notes preserved\n'
