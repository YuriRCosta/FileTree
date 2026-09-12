#!/usr/bin/env bash
set -euo pipefail
[[ $(hostname) == omarchy-test ]] || { printf '%s\n' 'Run only in the assigned Omarchy guest' >&2; exit 1; }
[[ $# == 4 ]] || { printf '%s\n' 'usage: 90-delivery-payload.sh SOURCE BACKEND TARGET NOTICES' >&2; exit 1; }
source_root=$(realpath -e -- "$1")
native=$source_root/tools/native
work=$(mktemp -d /tmp/fileblade-delivery-90.XXXXXX)
trap 'rm -rf -- "$work"' EXIT
"$native" stage "$source_root" "$2" "$3" "$4" "$work/payload"
payload=$work/payload
"$native" check "$payload"
printf 'PASS E-90-01 complete inventory and compatible runtime\n'
reject() {
  if "$native" "$1" "$2" >"$work/rejection" 2>&1; then
    printf 'FAIL %s: invalid payload accepted\n' "$3" >&2
    exit 1
  fi
  cat "$work/rejection"
  printf 'PASS %s\n' "$3"
}
cp -- "$payload/Service.qml" "$work/service"
printf '\nmodified\n' >> "$payload/Service.qml"
reject verify "$payload" E-90-02-digest
cp -- "$work/service" "$payload/Service.qml"
chmod 600 "$payload/Service.qml"
reject verify "$payload" E-90-02-mode
chmod 644 "$payload/Service.qml"
touch "$payload/unrecorded"
reject verify "$payload" E-90-02-extra
rm -- "$payload/unrecorded"
mv -- "$payload/Service.qml" "$work/removed"
reject verify "$payload" E-90-02-missing
ln -s -- "$work/removed" "$payload/Service.qml"
reject verify "$payload" E-90-02-symlink
rm -- "$payload/Service.qml"
mv -- "$work/removed" "$payload/Service.qml"
cp -- "$payload/payload.json" "$work/manifest"
jq '.files += [.files[0]]' "$work/manifest" > "$payload/payload.json"
reject verify "$payload" E-90-03-duplicate
jq '.files[0].path = "../escape"' "$work/manifest" > "$payload/payload.json"
reject verify "$payload" E-90-03-traversal
jq '.architecture = "aarch64" | .target = "aarch64-unknown-linux-musl"' "$work/manifest" > "$payload/payload.json"
reject verify "$payload" E-90-04-architecture
cp -- "$work/manifest" "$payload/payload.json"
mkdir "$work/path"
while IFS= read -r dependency; do
  [[ $dependency == wl-paste ]] || ln -s -- "$(command -v "$dependency")" "$work/path/$dependency"
done < <(jq -r '.commands[]' "$source_root/packaging/runtime.json")
PATH=$work/path reject check "$payload" E-90-05-missing-dependency
"$native" verify "$payload"
printf 'PASS E-90-06 restored payload accepted\n'
