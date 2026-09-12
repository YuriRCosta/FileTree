#!/usr/bin/env bash
set -euo pipefail
[[ $(hostname) == omarchy-test ]] || { printf '%s\n' 'Run only in the assigned Omarchy guest' >&2; exit 1; }
[[ $# == 2 ]] || { printf '%s\n' 'usage: 92-delivery-package.sh SOURCE PAYLOAD' >&2; exit 1; }
source_root=$(realpath -e -- "$1")
payload=$(realpath -e -- "$2")
native=$payload/tools/native
as_root() { sudo -S -p '' -- "$@" <<< omarchy; }
if pacman -Q fileblade-native >/dev/null 2>&1; then printf '%s\n' 'Refusing to replace an existing package fixture' >&2; exit 1; fi
work=$(mktemp -d /tmp/fileblade-delivery-92.XXXXXX)
package_installed=0
trap 'if [[ $package_installed == 1 ]] && pacman -Q fileblade-native >/dev/null 2>&1; then as_root pacman -R --noconfirm fileblade-native; fi; rm -rf -- "$work"' EXIT
cp -a -- "$payload" "$work/private-payload"
chmod 700 "$work/private-payload"
"$source_root/packaging/build" "$work/private-payload" "$work/package"
packages=("$work/package"/*.pkg.tar.*)
[[ ${#packages[@]} == 1 ]]
package=${packages[0]}
mkdir "$work/unpacked"
bsdtar -xf "$package" -C "$work/unpacked"
"$native" verify "$work/unpacked/usr/lib/fileblade"
cmp -- "$payload/payload.json" "$work/unpacked/usr/lib/fileblade/payload.json"
[[ $(stat -c %a "$work/unpacked/usr/lib/fileblade") == 755 ]]
jq -r '.packages[]' "$payload/packaging/runtime.json" | sort > "$work/expected-dependencies"
sed -n 's/^depend = //p' "$work/unpacked/.PKGINFO" | sort > "$work/actual-dependencies"
cmp -- "$work/expected-dependencies" "$work/actual-dependencies"
[[ ! -e $work/unpacked/.INSTALL ]]
printf 'PASS E-92-01 package preserves payload and declared dependencies without hooks\n'
export HOME=$work/home XDG_DATA_HOME=$work/data XDG_CONFIG_HOME=$work/config XDG_STATE_HOME=$work/state
mkdir -p "$HOME" "$XDG_CONFIG_HOME/xdg-desktop-portal"
printf 'keep file manager\n' > "$XDG_CONFIG_HOME/mimeapps.list"
printf 'keep chooser\n' > "$XDG_CONFIG_HOME/xdg-desktop-portal/portals.conf"
"$native" install "$payload"
installation=$XDG_DATA_HOME/fileblade/installation
cp -- "$installation/active/receipt.json" "$work/receipt"
package_installed=1
as_root pacman -U --noconfirm "$package"
[[ $(pacman -Qoq /usr/bin/fileblade) == fileblade-native && $(pacman -Qoq /usr/lib/fileblade/target/release/fileblade) == fileblade-native ]]
"/usr/lib/fileblade/tools/native" verify /usr/lib/fileblade
cmp -- "$payload/payload.json" /usr/lib/fileblade/payload.json
printf 'PASS E-92-02 installed files have pacman ownership and original inventory\n'
exec 8</usr/share/fileblade-native/lock
flock -s 8
if as_root pacman -R --noconfirm fileblade-native > "$work/busy" 2>&1; then exit 1; fi
grep -F 'Checking FileBlade native runtime is idle' "$work/busy"
pacman -Q fileblade-native
flock -u 8
exec 8<&-
printf 'PASS E-92-05 package preflight refuses a held runtime lock\n'
if "$native" install "$payload" > "$work/collision" 2>&1; then exit 1; fi
if "$native" remove > "$work/remove-collision" 2>&1; then exit 1; fi
grep -F 'package-owned installation (fileblade-native): use pacman' "$work/remove-collision"
grep -F 'package-owned installation (fileblade-native): use pacman' "$work/collision"
cmp -- "$work/receipt" "$installation/active/receipt.json"
"$native" verify /usr/lib/fileblade
[[ -L $HOME/.local/bin/fileblade && $(readlink "$HOME/.local/bin/fileblade") == "$installation/launcher" ]]
printf 'PASS E-92-03 direct update refuses package collision without changing either installation\n'
as_root pacman -R --noconfirm fileblade-native
package_installed=0
[[ ! -e /usr/bin/fileblade && ! -e /usr/bin/fileblade-bin && ! -e /usr/lib/fileblade ]]
"$native" status
cmp -- "$work/receipt" "$installation/active/receipt.json"
[[ $(cat "$XDG_CONFIG_HOME/mimeapps.list") == 'keep file manager' && $(cat "$XDG_CONFIG_HOME/xdg-desktop-portal/portals.conf") == 'keep chooser' ]]
printf 'PASS E-92-04 package removal preserves direct installation and personal defaults\n'
sha256sum "$package" "$payload/payload.json"
