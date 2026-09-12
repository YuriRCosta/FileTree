#!/usr/bin/env bash
set -euo pipefail
[[ $(hostname) == omarchy-test ]] || { printf '%s\n' 'Run only in the assigned Omarchy guest' >&2; exit 1; }
[[ $# == 3 ]] || { printf '%s\n' 'usage: 95-delivery-installed.sh SOURCE CURRENT_PAYLOAD PREVIOUS_PAYLOAD' >&2; exit 1; }
source_root=$(realpath -e -- "$1")
payload=$(realpath -e -- "$2")
previous=$(realpath -e -- "$3")
native=$payload/tools/native
work=$(mktemp -d /home/omarchy/cinder-r56.XXXXXX)
printf '%s\n' "$work" > /home/omarchy/cinder-r56-fixture
export HOME=$work/home XDG_DATA_HOME=$work/data XDG_CONFIG_HOME=$work/config XDG_STATE_HOME=$work/state
export PATH=/usr/bin:/bin FILEBLADE_SPIKE_HOME=$work/spike
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME"
printf 'keep settings\n' > "$XDG_CONFIG_HOME/settings"
printf 'keep Notes\n' > "$XDG_STATE_HOME/notes"
current_digest=$(sha256sum "$payload/payload.json"); current_digest=${current_digest%% *}
previous_digest=$(sha256sum "$previous/payload.json"); previous_digest=${previous_digest%% *}
[[ $current_digest != "$previous_digest" ]]
installation=$XDG_DATA_HOME/fileblade/installation
"$native" install "$previous"
[[ $(jq -r .payload "$installation/active/receipt.json") == "$previous_digest" ]]
"$native" install "$payload"
[[ $(jq -r .payload "$installation/active/receipt.json") == "$current_digest" && $(jq -r .previous "$installation/active/receipt.json") == "$previous_digest" ]]
[[ $(readlink "$HOME/.local/bin/fileblade") == "$installation/launcher" ]]
"$native" verify "$installation/active/runtime"
printf 'PASS E-95-01 actual previous-to-current update and stable launcher receipt\n'
cp "$installation/active/receipt.json" "$work/receipt.json"
"$native" install "$payload"
cmp "$work/receipt.json" "$installation/active/receipt.json"
printf 'PASS E-95-02 identical-payload update preserves receipt\n'
"$native" rollback
[[ $(jq -r .payload "$installation/active/receipt.json") == "$previous_digest" ]]
"$native" verify "$installation/active/runtime"
"$native" install "$payload"
printf 'PASS E-95-03 rollback to actual previous payload and current restoration\n'
"$native" remove
[[ ! -L $HOME/.local/bin/fileblade && ! -e $installation/versions/$current_digest && -L $installation/removed ]]
[[ $(cat "$XDG_CONFIG_HOME/settings") == 'keep settings' && $(cat "$XDG_STATE_HOME/notes") == 'keep Notes' ]]
printf 'PASS E-95-04 offline removal preserves receipt and personal data\n'
"$native" install "$previous"
"$native" install "$payload"
runtime=$(readlink -f "$installation/active/runtime")
printf '%s\n' "$runtime" > "$work/runtime"
mv "$source_root" "$source_root.offline"
setsid "$HOME/.local/bin/fileblade" > "$work/native.log" 2>&1 </dev/null &
for attempt in {1..60}; do
  if timeout 2 qs ipc -n -p "$runtime/app" call fileblade.native status > "$work/native-status.json" 2>/dev/null; then
    if jq -e --arg root "$runtime" '.loaded and .sourceDir == $root' "$work/native-status.json"; then break; fi
  fi
  sleep 0.5
done
jq -e --arg root "$runtime" '.loaded and .sourceDir == $root' "$work/native-status.json"
timeout 8 qs ipc -n -p "$runtime/app" call data-goblin.fileblade status > "$work/catalog.json"
jq -e '.bladeModules | length == 8' "$work/catalog.json"
for helper in agent-skillsctl agent-memoryctl agent-hooksctl agent-mcpctl; do "$runtime/python/bin/$helper" --help > "$work/$helper.txt"; done
timeout 8 qs ipc -n -p "$runtime/app" call data-goblin.fileblade.control toggleBladeFocus left
printf 'PASS E-95-05 installed native launch, eight modules and core helper resolution\n'
cp "$installation/active/receipt.json" "$work/live-receipt.json"
for action in install rollback remove; do
  args=("$action")
  [[ $action != install ]] || args+=("$payload")
  if "$native" "${args[@]}" > "$work/live-$action.log" 2>&1; then
    printf 'FAIL live %s changed a running spike installation\n' "$action" >&2
    exit 1
  fi
  grep -F 'FileBlade is running' "$work/live-$action.log"
  cmp "$work/live-receipt.json" "$installation/active/receipt.json"
  printf 'PENDING live %s: runtime drain unavailable; busy refusal preserves activation\n' "$action"
done
timeout 8 qs ipc -n -p "$runtime/app" call fileblade.native status > "$work/after-status.json"
jq -e '.loaded' "$work/after-status.json"
printf 'PASS E-95-06 resident maintenance refusals retain receipt and running app\n'
printf 'Retained fixture: %s\n' "$work"
