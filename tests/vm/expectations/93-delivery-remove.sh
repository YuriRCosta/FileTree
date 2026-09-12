#!/usr/bin/env bash
set -euo pipefail
[[ $(hostname) == omarchy-test ]] || { printf '%s\n' 'Run only in the assigned Omarchy guest' >&2; exit 1; }
[[ $# == 1 ]] || { printf '%s\n' 'usage: 93-delivery-remove.sh PAYLOAD' >&2; exit 1; }
payload=$(realpath -e -- "$1")
native=$payload/tools/native
work=$(mktemp -d /tmp/fileblade-delivery-93.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
export HOME=$work/home XDG_DATA_HOME=$work/data XDG_CONFIG_HOME=$work/config XDG_STATE_HOME=$work/state
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME"
printf 'keep Notes\n' > "$XDG_STATE_HOME/notes"
printf 'keep defaults\n' > "$XDG_CONFIG_HOME/mimeapps.list"
"$native" remove
"$native" install "$payload"
installation=$XDG_DATA_HOME/fileblade/installation
first=$(jq -r .payload "$installation/active/receipt.json")
cp -a "$payload" "$work/update"
printf '\n' >> "$work/update/payload.json"
"$native" install "$work/update"
second=$(jq -r .payload "$installation/active/receipt.json")
mv -- "$installation/versions/$second" "$work/missing"
if "$native" rollback > "$work/refusal" 2>&1; then exit 1; fi
[[ $(jq -r .payload "$installation/active/receipt.json") == "$second" ]]
mv -- "$work/missing" "$installation/versions/$second"
"$native" rollback
[[ $(jq -r .payload "$installation/active/receipt.json") == "$first" ]]
printf 'PASS E-93-01 missing active payload refuses until verified runtime is restored\n'
activation=$(readlink "$installation/active")
rm -- "$installation/active"
if "$native" install "$payload" > "$work/refusal" 2>&1; then exit 1; fi
[[ ! -L $installation/active ]]
ln -s "$activation" "$installation/active"
"$native" install "$payload"
[[ $(jq -r .payload "$installation/active/receipt.json") == "$first" ]]
printf 'PASS E-93-02 missing activation refuses until owned pointer is restored\n'
cp "$installation/launcher" "$work/launcher"
printf '\nchanged\n' >> "$installation/launcher"
if "$native" remove > "$work/refusal" 2>&1; then exit 1; fi
cp "$work/launcher" "$installation/launcher"
printf 'unowned\n' > "$installation/versions/$first/extra"
if "$native" remove > "$work/refusal" 2>&1; then exit 1; fi
[[ -L $installation/active && $(cat "$installation/versions/$first/extra") == unowned ]]
rm "$installation/versions/$first/extra"
printf 'PASS E-93-03 modified launcher and payload preserved\n'
exec 8>"$installation/lock"
flock -s 8
if "$native" remove > "$work/refusal" 2>&1; then exit 1; fi
flock -u 8
exec 8>&-
printf 'PASS E-93-04 removal refuses live installation lock\n'
mkdir "$work/path"
cat > "$work/path/rm" <<'EOF'
#!/usr/bin/env bash
for argument in "$@"; do
  if [[ $argument == */installation/discard ]]; then
    /usr/bin/find "$argument" -name Service.qml -delete
    kill -KILL -- "-$(ps -o pgid= -p $$ | tr -d ' ')"
  fi
done
exec /usr/bin/rm "$@"
EOF
chmod 755 "$work/path/rm"
if setsid env PATH="$work/path:$PATH" "$native" remove > "$work/interrupted" 2>&1; then exit 1; fi
[[ -L $installation/removing && ! -L $installation/active && -d $installation/discard ]]
if "$native" install "$payload" > "$work/refusal" 2>&1; then exit 1; fi
"$native" remove
"$native" remove
[[ -L $installation/removed && ! -e $installation/discard && ! -e $installation/versions/$first ]]
[[ ! -e $HOME/.local/bin/fileblade && ! -L $HOME/.local/bin/fileblade ]]
[[ $(cat "$XDG_STATE_HOME/notes") == 'keep Notes' && $(cat "$XDG_CONFIG_HOME/mimeapps.list") == 'keep defaults' ]]
printf 'PASS E-93-05 interrupted deletion resumes with receipt and user data retained\n'
"$native" install "$work/update"
activation=$(readlink "$installation/active")
cp "$installation/active/receipt.json" "$work/second-receipt"
rm "$installation/active"
if "$native" install "$payload" > "$work/stale-install" 2>&1; then cat "$work/stale-install"; exit 1; fi
if "$native" remove > "$work/stale-remove" 2>&1; then cat "$work/stale-remove"; exit 1; fi
cmp "$work/second-receipt" "$installation/$activation/receipt.json"
[[ -d $installation/versions/$second && ! -L $installation/active ]]
ln -s "$activation" "$installation/active"
printf 'PASS E-93-07 old removal receipt cannot bypass missing reinstallation identity\n'
"$native" remove
printf 'PASS E-93-06 reinstall and subsequent removal succeed\n'
