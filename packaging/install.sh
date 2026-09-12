private_directory() {
  [[ ! -L $1 ]] || fail "installation directory is a symlink: $1"
  if [[ ! -e $1 ]]; then mkdir -m 700 -- "$1"; fi
  [[ -d $1 && $(stat -c %u:%a -- "$1") == "$(id -u):700" ]] || fail "installation directory must be owned and private: $1"
}

launcher_text() {
  printf '#!/usr/bin/env bash\nset -euo pipefail\n'
  printf 'installation=%q\n' "$installation"
  printf '%s\n' \
    'exec 9>"$installation/lock"' \
    'flock -s 9' \
    'runtime=$(readlink -f -- "$installation/active/runtime")' \
    '[[ $runtime == "$installation/versions/"* && -x $runtime/app/launch ]] || { printf "%s\n" "fileblade: activation unavailable; use tools/native rollback" >&2; exit 1; }' \
    'export FILEBLADE_APP_ROOT=$runtime' \
    'exec "$runtime/app/launch" "$@"'
}

installation_paths() {
  [[ ${HOME:-} == /* && $HOME != / ]] || fail 'HOME must be an absolute user directory'
  local data=${XDG_DATA_HOME:-$HOME/.local/share}
  [[ $data == /* ]] || data=$HOME/.local/share
  installation=$(realpath -m -- "$data/fileblade/installation")
  launcher=$HOME/.local/bin/fileblade
  [[ $(realpath -m -- "$data/fileblade") == "$data/fileblade" ]] || fail 'installation parent contains a symlink or noncanonical component'
  [[ $(realpath -m -- "$HOME/.local/bin") == "$HOME/.local/bin" ]] || fail 'launcher parent contains a symlink or noncanonical component'
}

check_owner() {
  local path package
  for path in /usr/bin/fileblade /usr/bin/fileblade-bin /usr/lib/fileblade "$launcher"; do
    if [[ -e $path || -L $path ]]; then
      package=$(pacman -Qoq -- "$path" 2>/dev/null) && fail "package-owned installation ($package): use pacman"
    fi
  done
  if [[ -e $launcher || -L $launcher ]]; then
    [[ -L $launcher && $(readlink -- "$launcher") == "$installation/launcher" ]] || fail "unrelated file occupies $launcher"
  fi
  if [[ -e $installation/launcher || -L $installation/launcher ]]; then
    [[ -f $installation/launcher && ! -L $installation/launcher ]] || fail 'invalid owned launcher'
    [[ $(launcher_text) == "$(cat -- "$installation/launcher")" ]] || fail 'owned launcher was changed'
  elif [[ -e $installation/active || -L $launcher ]]; then
    fail 'installation launcher is missing'
  fi
}

read_activation() {
  active_payload=
  previous_payload=
  [[ -e $installation/active || -L $installation/active ]] || return 0
  local link receipt
  [[ -L $installation/active ]] || fail 'activation pointer is not owned'
  link=$(readlink -- "$installation/active")
  [[ $link =~ ^generations/generation\.[A-Za-z0-9]+$ ]] || fail 'invalid activation pointer'
  [[ -d $installation/$link && ! -L $installation/$link ]] || fail 'activation generation is missing'
  receipt=$installation/$link/receipt.json
  [[ -f $receipt && ! -L $receipt && $(stat -c %s -- "$receipt") -le 16384 ]] || fail 'receipt is missing or invalid'
  jq -e --arg root "$installation" '
    .schema == 1 and .owner == "direct" and .installation == $root and
    (.payload | type == "string" and test("^[a-f0-9]{64}$")) and
    (.previous == null or (.previous | type == "string" and test("^[a-f0-9]{64}$")))
  ' "$receipt" >/dev/null || fail 'receipt ownership is invalid'
  active_payload=$(jq -r .payload "$receipt")
  previous_payload=$(jq -r '.previous // ""' "$receipt")
  [[ -L $installation/$link/runtime && $(readlink -- "$installation/$link/runtime") == "../../versions/$active_payload" ]] || fail 'receipt and runtime pointer disagree'
  [[ -d $installation/versions/$active_payload && ! -L $installation/versions/$active_payload ]] || fail 'active runtime is missing'
  [[ $(sha256sum -- "$installation/versions/$active_payload/payload.json") == "$active_payload "* ]] || fail 'active manifest identity differs'
}

activate_payload() (
  local payload=$1 previous=$2 stage generation pointer
  stage=$(mktemp -d "$installation/generations/.generation.XXXXXX")
  pointer=
  trap 'rm -rf -- "$stage"; [[ -z $pointer ]] || rm -f -- "$pointer"' EXIT
  generation=${stage##*/}
  generation=${generation#.}
  jq -n --arg root "$installation" --arg payload "$payload" --arg previous "$previous" \
    '{schema: 1, owner: "direct", installation: $root, payload: $payload, previous: (if $previous == "" then null else $previous end)}' > "$stage/receipt.json"
  chmod 600 "$stage/receipt.json"
  ln -s -- "../../versions/$payload" "$stage/runtime"
  sync -f -- "$stage/receipt.json"
  sync -f -- "$stage"
  mv -T -n -- "$stage" "$installation/generations/$generation"
  [[ ! -d $stage ]] || fail 'activation generation collision'
  sync -f -- "$installation/generations"
  pointer=$installation/.active.${generation#*.}
  ln -s -- "generations/$generation" "$pointer"
  mv -Tf -- "$pointer" "$installation/active"
  sync -f -- "$installation"
)

install_payload() (
  local source=$1 digest destination stage
  digest=$(sha256sum -- "$source/payload.json")
  digest=${digest%% *}
  destination=$installation/versions/$digest
  if [[ -e $destination || -L $destination ]]; then
    [[ -d $destination && ! -L $destination ]] || fail 'runtime destination is not owned'
    verify_payload "$destination"
    [[ $(sha256sum -- "$destination/payload.json") == "$digest "* ]] || fail 'stored manifest identity differs'
    check_runtime "$destination"
  else
    stage=$(mktemp -d "$installation/versions/.payload.XXXXXX")
    trap 'rm -rf -- "$stage"' EXIT
    cp -a -- "$source/." "$stage/"
    chmod 755 "$stage"
    verify_payload "$stage"
    [[ $(sha256sum -- "$stage/payload.json") == "$digest "* ]] || fail 'payload changed during staging'
    check_runtime "$stage"
    sync -f -- "$stage"
    mv -T -n -- "$stage" "$destination"
    [[ ! -d $stage ]] || fail 'runtime appeared during staging'
    sync -f -- "$installation/versions"
  fi
  if [[ $digest != "$active_payload" ]]; then activate_payload "$digest" "$active_payload"; fi
  if [[ ! -L $launcher ]]; then
    ln -s -- "$installation/launcher" "$launcher"
    sync -f -- "${launcher%/*}"
  fi
  printf 'Installed %s\nLauncher: %s\nReceipt: %s\n' "$digest" "$launcher" "$installation/active/receipt.json"
)

install_command() (
  local action=$1 installation launcher active_payload previous_payload source pending_launcher
  case $action in
    install) [[ $# == 2 ]] || fail 'usage: tools/native install PAYLOAD'; source=$(realpath -e -- "$2") ;;
    rollback|status) [[ $# == 1 ]] || fail "usage: tools/native $action" ;;
  esac
  installation_paths
  command -v pacman >/dev/null || fail 'installation ownership checks require the tested Arch/Omarchy package database'
  check_owner
  if [[ $action == install ]]; then
    verify_payload "$source"
    check_runtime "$source"
    mkdir -p -- "${installation%/*}" "${launcher%/*}"
    private_directory "$installation"
  else
    [[ -d $installation ]] || fail 'no direct installation'
    private_directory "$installation"
  fi
  [[ ! -L $installation/lock && ( ! -e $installation/lock || -f $installation/lock ) ]] || fail 'invalid installation lock'
  exec 9>"$installation/lock"
  if [[ $action == status ]]; then
    flock -n -s 9 || fail 'another installer holds the lock'
  else
    flock -n -x 9 || fail 'FileBlade is running or another installer holds the lock; close the native session before updating'
  fi
  check_owner
  private_directory "$installation/versions"
  private_directory "$installation/generations"
  read_activation
  case $action in
    status) [[ -n $active_payload ]] || fail 'no active installation'; cat -- "$installation/active/receipt.json" ;;
    rollback)
      [[ -n $previous_payload ]] || fail 'no previous runtime to recover'
      verify_payload "$installation/versions/$previous_payload"
      check_runtime "$installation/versions/$previous_payload"
      activate_payload "$previous_payload" "$active_payload"
      printf 'Restored %s\n' "$previous_payload"
      ;;
    install)
      if [[ ! -f $installation/launcher ]]; then
        pending_launcher=$(mktemp "$installation/.launcher.XXXXXX")
        trap 'rm -f -- "$pending_launcher"' EXIT
        launcher_text > "$pending_launcher"
        chmod 755 "$pending_launcher"
        sync -f -- "$pending_launcher"
        mv -T -n -- "$pending_launcher" "$installation/launcher"
        [[ ! -e $pending_launcher ]] || fail 'launcher appeared during staging'
      fi
      install_payload "$source"
      ;;
  esac
)
