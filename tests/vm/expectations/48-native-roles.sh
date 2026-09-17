#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"

[[ $FILEBLADE_SHAPE == native ]] || { printf '%s\n' 'FILEBLADE_SHAPE=native is required: desktop roles exist only in the installed app' >&2; exit 1; }
require_guest

launcher=$(guest 'if test -x "$HOME/.local/bin/fileblade"; then printf %s "$HOME/.local/bin/fileblade"; else printf /usr/bin/fileblade; fi')
fixture=/tmp/fileblade-roles-$$
config=$fixture/config
data=$fixture/data
mimeapps=$config/mimeapps.list
portals=$config/xdg-desktop-portal/portals.conf
owner_pid=""

roles() {
  guest "env XDG_CONFIG_HOME=$(printf '%q' "$config") XDG_DATA_HOME=$(printf '%q' "$data") $(printf '%q' "$launcher") native roles $* --json"
}
gio_default() {
  guest "env XDG_CONFIG_HOME=$(printf '%q' "$config") XDG_DATA_HOME=$(printf '%q' "$data") gio mime inode/directory 2>/dev/null | sed -n 's/^Default application for .inode\\/directory.: //p'"
}
guest_file() { guest "cat $(printf '%q' "$1") 2>/dev/null"; }
guest_exists() { guest "test -e $(printf '%q' "$1") && echo yes || echo no"; }
role_field() { jq -r --arg role "$1" --arg key "$2" '.roles[$role][$key]' <<<"$3"; }

cleanup() {
  [[ -n $owner_pid ]] && guest "kill -TERM $owner_pid >/dev/null 2>&1 || true"
  roles disable --all >/dev/null 2>&1 || true
  guest "rm -rf $(printf '%q' "$fixture")"
}
trap cleanup EXIT

guest "mkdir -p $(printf '%q' "$data/applications") $(printf '%q' "$config/xdg-desktop-portal") $(printf '%q' "$fixture/backup")"
guest "printf '[Desktop Entry]\nType=Application\nName=Prior\nExec=true %%U\nMimeType=inode/directory;\n' > $(printf '%q' "$data/applications/prior.desktop")"
guest "printf '[Desktop Entry]\nType=Application\nName=Other\nExec=true %%U\nMimeType=inode/directory;\n' > $(printf '%q' "$data/applications/other.desktop")"
guest "printf '[Added Associations]\ntext/plain=prior.desktop;\n\n[Default Applications]\ninode/directory=prior.desktop\ntext/plain=prior.desktop\n' > $(printf '%q' "$mimeapps")"
guest "printf '[preferred]\ndefault=gtk\n' > $(printf '%q' "$portals")"
guest "cp -a $(printf '%q' "$mimeapps") $(printf '%q' "$fixture/backup/mimeapps.list"); cp -a $(printf '%q' "$portals") $(printf '%q' "$fixture/backup/portals.conf")"

status=$(roles status)
expect_true E-40-12 "every role is off after install" "[[ \$(jq -r '[.roles[] | .enabled] | all(. == false)' <<<\"\$status\") == true ]]"
expect_true E-40-12 "status lists exactly the five roles" "[[ \$(jq -r '.roles | keys | join(\",\")' <<<\"\$status\") == autostart,bindings,chooser,folder,reveal ]]"
expect_true E-40-12 "the prior handler still opens folders" "[[ \$(gio_default) == prior.desktop ]]"
expect_true E-40-12 "no reveal service is registered" "[[ \$(guest_exists \"$data/dbus-1/services/org.freedesktop.FileManager1.service\") == no ]]"
expect_true E-40-12 "the chooser routing is untouched" "guest \"cmp -s $(printf '%q' "$portals") $(printf '%q' "$fixture/backup/portals.conf")\""

result=$(roles enable --role folder)
expect_true E-40-13 "enabling folder opening succeeds" "[[ \$(jq -r .status <<<\"\$result\") == complete ]]"
expect_true E-40-13 "folders now open with FileBlade" "[[ \$(gio_default) == fileblade.desktop ]]"
desktop=$(guest_file "$data/applications/fileblade.desktop")
expect_contains E-40-13 "the desktop entry runs the stable launcher" "$desktop" "Exec=$launcher native open %U"
expect_contains E-40-13 "other associations are kept" "$(guest_file "$mimeapps")" "text/plain=prior.desktop"
expect_true E-40-13 "status reports the role on" "[[ \$(role_field folder enabled \"\$(roles status)\") == true ]]"

result=$(roles enable --role chooser)
expect_true E-40-14 "enabling the chooser succeeds" "[[ \$(jq -r .status <<<\"\$result\") == complete ]]"
portal_text=$(guest_file "$portals")
expect_contains E-40-14 "portals.conf prefers FileBlade for FileChooser" "$portal_text" "org.freedesktop.impl.portal.FileChooser=fileblade"
expect_contains E-40-14 "portals.conf keeps the default route" "$portal_text" "default=gtk"
expect_contains E-40-14 "the portal descriptor names the FileBlade bus name" "$(guest_file "$data/xdg-desktop-portal/portals/fileblade.portal")" "DBusName=org.freedesktop.impl.portal.desktop.fileblade"
expect_contains E-40-14 "the portal service activates the launcher" "$(guest_file "$data/dbus-1/services/org.freedesktop.impl.portal.desktop.fileblade.service")" "Exec=$launcher native portal"

result=$(roles disable --role folder)
expect_true E-40-15 "turning folder opening off restores the prior handler" "[[ \$(role_field folder status \"\$result\") == restored ]]"
expect_true E-40-15 "mimeapps.list is byte-identical to before" "guest \"cmp -s $(printf '%q' "$mimeapps") $(printf '%q' "$fixture/backup/mimeapps.list")\""
expect_true E-40-15 "the desktop entry is gone" "[[ \$(guest_exists \"$data/applications/fileblade.desktop\") == no ]]"
expect_true E-40-15 "folders open with the prior handler again" "[[ \$(gio_default) == prior.desktop ]]"
result=$(roles disable --role chooser)
expect_true E-40-15 "turning the chooser off restores portals.conf" "guest \"cmp -s $(printf '%q' "$portals") $(printf '%q' "$fixture/backup/portals.conf")\""
expect_true E-40-15 "the portal descriptor is gone" "[[ \$(guest_exists \"$data/xdg-desktop-portal/portals/fileblade.portal\") == no ]]"

roles enable --role folder >/dev/null
guest "sed -i 's/^inode\\/directory=fileblade.desktop$/inode\\/directory=other.desktop/' $(printf '%q' "$mimeapps")"
result=$(roles disable --role folder)
expect_true E-40-16 "a handler changed since enabling is kept" "[[ \$(role_field folder status \"\$result\") == preserved_newer ]]"
expect_true E-40-16 "the newer handler still opens folders" "[[ \$(gio_default) == other.desktop ]]"
expect_true E-40-16 "the role reads as off" "[[ \$(role_field folder enabled \"\$(roles status)\") == false ]]"

if guest 'python3 -c "import gi; gi.require_version(\"Gio\", \"2.0\")" >/dev/null 2>&1'; then
  owner_pid=$(guest "setsid python3 -c 'import gi; gi.require_version(\"Gio\", \"2.0\"); from gi.repository import Gio, GLib; Gio.bus_own_name(Gio.BusType.SESSION, \"org.freedesktop.FileManager1\", Gio.BusNameOwnerFlags.NONE, None, None, None); GLib.MainLoop().run()' >/dev/null 2>&1 < /dev/null & echo \$!")
  wait_for "guest 'gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.NameHasOwner org.freedesktop.FileManager1' | grep -q true" 10
  result=$(roles enable --role reveal)
  expect_true E-40-17 "reveal turns on beside the current owner" "[[ \$(role_field reveal enabled \"\$(roles status)\") == true ]]"
  expect_contains E-40-17 "the conflict names the owner and asks for a fresh login" "$(role_field reveal conflict "$(roles status)")" "currently owns org.freedesktop.FileManager1; log out and in for FileBlade to take over"
  guest "kill -TERM $owner_pid >/dev/null 2>&1 || true"
  owner_pid=""
else
  pending E-40-17 "the conflict message when another file manager owns reveal" "python3 gi is unavailable in the guest"
fi

roles enable --role autostart >/dev/null
result=$(roles disable --all)
expect_true E-40-18 "disable --all reverses every owned entry" "[[ \$(jq -r '.status, .error, (.remaining_owned_entries | length)' <<<\"\$result\" | paste -sd,) == complete,,0 ]]"
expect_true E-40-18 "every role reports restored or already off" "[[ \$(jq -r '[.roles[] | .status] | all(. == \"restored\" or . == \"already_off\" or . == \"preserved_newer\")' <<<\"\$result\") == true ]]"
expect_true E-40-18 "no owned file remains" "[[ \$(guest_exists \"$data/dbus-1/services/org.freedesktop.FileManager1.service\")\$(guest_exists \"$config/autostart/fileblade.desktop\") == nono ]]"
installer_document=$(jq -c -n '{schema:1, action:"roles_disable", status:"complete", error:"", remaining_owned_entries:[], roles:(["autostart","bindings","chooser","folder","reveal"] | map({key:.,value:{status:"already_off",remaining_owned_entries:[],error:""}}) | from_entries)}')
expect_true E-40-18 "a second disable --all is the installer's exact document" "[[ \$(roles disable --all | jq -c -S .) == \$(jq -c -S . <<<\"\$installer_document\") ]]"

summary
