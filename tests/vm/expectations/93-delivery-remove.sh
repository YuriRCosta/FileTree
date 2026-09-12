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
rm -rf -- "$installation/versions/$second"
"$native" rollback
[[ $(jq -r .payload "$installation/active/receipt.json") == "$first" ]]
printf 'PASS E-93-01 rollback repairs a missing active payload\n'
rm -- "$installation/active"
"$native" install "$payload"
[[ $(jq -r .payload "$installation/active/receipt.json") == "$first" ]]
printf 'PASS E-93-02 explicit local install repairs missing activation\n'
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
"$native" install "$payload"
"$native" remove
printf 'PASS E-93-06 reinstall and subsequent removal succeed\n'
