#!/usr/bin/env bash
set -eu
source "$(dirname "$0")/lib.sh"

require_guest
PRODUCTION_ROOT=$GUEST_PLUGIN

pending_shot() {
  local name=$1
  local path
  path=$("$OVM" shot "$name" 2>/dev/null | tail -n 1 || true)
  if [[ -n $path ]]; then
    printf 'pending-shot %s  %s\n' "$name" "$path"
  else
    fail harness "$name screenshot" "the OVM shot verb returned no path"
  fi
}

resolver_interface=$(guest "grep -Fq 'function resolveApplication' '$GUEST_PLUGIN/lib/FileIcons.js' && echo yes || echo no" | tr -d '\r')
if [[ $resolver_interface != yes ]]; then
  pending E-26-04 "R31 real wheel row resolves a desktop launcher icon" "the installed guest lacks the merged FileIcons.resolveApplication and SafeApplicationIcon interface"
  pending E-38-03 "R31 explicit wheel imagery override wins" "the installed guest lacks the merged FileIcons.resolveApplication and SafeApplicationIcon interface"
  pending E-38-03 "R31 missing desktop entry falls back to a bundled mark" "the installed guest lacks the merged FileIcons.resolveApplication and SafeApplicationIcon interface"
  pending E-38-03 "R31 failed wheel image displays its fallback glyph" "the installed guest lacks the merged FileIcons.resolveApplication and SafeApplicationIcon interface"
  pending_shot E-26-04-R31-nvim
  pending_shot E-38-03-R31-override
  pending_shot E-38-03-R31-missing-desktop
  pending_shot E-38-03-R31-missing-image
  summary
fi

SCRIPT_DIR=$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
COMMONS_SOURCE=$SCRIPT_DIR/../../imports/qs/Commons
if [[ ! -d $COMMONS_SOURCE ]]; then
  fail harness "prepare the real Quickshell imports" "the host fixture is absent: $COMMONS_SOURCE"
  summary
fi
printf 'note R31 disposable probe uses host fixture %s and production UI from guest %s\n' "$COMMONS_SOURCE" "$PRODUCTION_ROOT"

PROBE_DIR=$(guest "mktemp -d /tmp/fb-r31-wheel-icons.XXXXXX" | tr -d '\r')
PROBE_PID=
if [[ ! $PROBE_DIR =~ ^/tmp/fb-r31-wheel-icons\.[[:alnum:]]+$ ]]; then
  fail harness "prepare the disposable wheel icon probe" "the guest temporary path is invalid: $PROBE_DIR"
  summary
fi
PROBE_PATH=$PROBE_DIR/probe.qml
PROBE_LOG=$PROBE_DIR/probe.log
PROBE_PIDFILE=$PROBE_DIR/probe.pid

cleanup() {
  ctl hideDropWheel >/dev/null 2>&1 || true
  if [[ $PROBE_PID =~ ^[0-9]+$ ]]; then
    guest "kill $PROBE_PID" >/dev/null 2>&1 || true
  fi
  guest "rm -rf -- '$PROBE_DIR'" >/dev/null 2>&1 || true
}
trap cleanup EXIT

guest "mkdir -p '$PROBE_DIR/Commons'"
for fixture in "$COMMONS_SOURCE"/*; do
  fixture_name=${fixture##*/}
  fixture_data=$(base64 -w0 "$fixture")
  guest "printf '%s' $fixture_data | base64 -d > '$PROBE_DIR/Commons/$fixture_name'"
done

ctl hideDropWheel >/dev/null 2>&1 || true
probe=$(cat <<'QML'
import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {
  id: probe
  property int attempts: 0
  property bool finished: false
  property string sourceError: ""
  property var wheel: null
  property var wheelComponent: null

  QtObject {
    id: controller
    property bool wheelOpen: true
    property bool wheelFromDrag: false
    property var wheelScreen: null
    property real wheelX: 960
    property real wheelY: 500
    property int highlighted: -1
    property int parentIndex: -1
    property int subParentIndex: -1
    property bool dragOutside: false
    property var ringItems: [
      { id: "r31-nvim", label: "Neovim", key: "n", desktop_id: "nvim.desktop", icon: "r31-no-such-theme-icon", icon_source: "file:///tmp/r31-wheel-icons-stale.svg", icon_override: false, glyph: "N" },
      { id: "r31-override", label: "Explicit override", key: "o", desktop_id: "nvim.desktop", icon: "herdr", icon_source: "file:///tmp/r31-wheel-icons-stale.svg", icon_override: true, glyph: "O" },
      { id: "r31-missing", label: "Missing desktop", key: "m", desktop_id: "r31-no-such-entry.desktop", icon: "herdr", icon_override: false, glyph: "M" },
      { id: "r31-failed", label: "Missing image", key: "f", desktop_id: "r31-no-such-entry.desktop", icon: "", icon_source: "file:///tmp/r31-wheel-icons-missing.svg", icon_override: false, glyph: "Z" }
    ]
    property int count: ringItems.length
    property var outerItems: []
    property var subItems: []
    property bool loading: false
    property bool dragActive: false
    property bool dragDocked: false
    property var dragEntries: []
    property var dragScreen: null
    property string diagnostics: ""
    property string error: ""
    property real extent: 220
    property real gapRadius: 10
    property real hubRadius: 42
    property bool keyboardFocusReleased: false
    property real outerRadius: 180
    property string pathForm: ""
    property real pointerX: 0
    property real pointerY: 0
    property string ringTitle: "R31 wheel icons"
    property string status: ""
    property string targetLabel: "Desktop"
    property string toast: ""
    property bool toastError: false
    property real childInnerRadius: 0
    property real childOuterRadius: 0
    property real childStep: 0
    property real subInnerRadius: 0
    property real subOuterRadius: 0
    property real subStep: 0
    property bool outerFocus: false
    property int outerHighlighted: -1
    property int subHighlighted: -1

    function accept() {}
    function activate() {}
    function activateChild() {}
    function activateKey() { return false }
    function activateSub() {}
    function back() {}
    function childAngle() { return 0 }
    function childrenOf() { return [] }
    function clearPendingRelease() {}
    function close() { wheelOpen = false }
    function handleDragKey() { return false }
    function handleDragKeyRelease() { return false }
    function hasChildren() { return false }
    function hover() {}
    function moveHighlight() {}
    function pointAt() { return ({ ring: "none", index: -1 }) }
    function subAngle() { return 0 }
    function wedgeAngle(index) { return -Math.PI / 2 + index * 2 * Math.PI / ringItems.length }
  }

  Component.onCompleted: {
    wheelComponent = Qt.createComponent("PLUGIN_DROPWHEEL_URL", Component.PreferSynchronous)
    if (wheelComponent.status === Component.Error) sourceError = wheelComponent.errorString()
    else if (wheelComponent.status === Component.Ready) wheel = wheelComponent.createObject(probe, { controller: controller })
  }

  function descendants(item, result) {
    if (!item || result.indexOf(item) >= 0 || result.length >= 256) return
    result.push(item)
    var children = item.children || []
    for (var i = 0; i < children.length; i++) descendants(children[i], result)
    if (item.contentItem) descendants(item.contentItem, result)
    var data = item.data || []
    for (var j = 0; j < data.length; j++) descendants(data[j], result)
  }

  function rowId(item) {
    var current = item
    for (var depth = 0; current && depth < 12; depth++) {
      if (current.modelData && current.modelData.id !== undefined) return String(current.modelData.id)
      current = current.parent
    }
    return ""
  }

  function rowDelegate(item) {
    var current = item
    for (var depth = 0; current && depth < 12; depth++) {
      if (current.modelData && current.modelData.id !== undefined && current.applicationIcon !== undefined) return current
      current = current.parent
    }
    return null
  }

  function imageFor(item) {
    var children = item ? item.children || [] : []
    for (var i = 0; i < children.length; i++)
      if (children[i].sourceSize !== undefined && children[i].status !== undefined) return children[i]
    return null
  }

  function fallbackFor(item) {
    var children = item ? item.children || [] : []
    for (var i = 0; i < children.length; i++)
      if (children[i].textFormat !== undefined && children[i].text !== undefined) return children[i]
    return null
  }

  function record(id, icons) {
    for (var i = 0; i < icons.length; i++) {
      if (rowId(icons[i]) !== id) continue
      var delegate = rowDelegate(icons[i])
      var resolved = delegate ? delegate.applicationIcon || ({}) : ({})
      var image = imageFor(icons[i])
      var fallback = fallbackFor(icons[i])
      return {
        found: true,
        row_id: id,
        resolver_icon: String(resolved.icon || ""),
        resolver_source: String(resolved.icon_source || ""),
        resolver_glyph: String(resolved.glyph || ""),
        safe_icon_name: String(icons[i].iconName || ""),
        safe_trusted_source: String(icons[i].trustedIconSource || ""),
        safe_source: String(icons[i].resolvedSource || ""),
        image_source: image ? String(image.source || "") : "",
        image_status: image ? image.status : -1,
        image_ready: !!image && image.status === Image.Ready,
        image_error: !!image && image.status === Image.Error,
        descriptor_forwarded: icons[i].applicationDescriptor !== undefined && icons[i].applicationDescriptor !== null,
        fallback_visible: !!fallback && fallback.visible,
        fallback_text: fallback ? String(fallback.text || "") : ""
      }
    }
    return { found: false, row_id: id, resolver_icon: "", resolver_source: "", resolver_glyph: "", safe_icon_name: "", safe_trusted_source: "", safe_source: "", image_source: "", image_status: -1, image_ready: false, image_error: false, descriptor_forwarded: false, fallback_visible: false, fallback_text: "" }
  }

  function emitResult(value) {
    if (finished) return
    finished = true
    inspect.stop()
    console.log("R31_WHEEL_ICONS_RESULT " + JSON.stringify(value))

  }

  Timer {
    id: inspect
    interval: 100
    repeat: true
    running: true
    onTriggered: {
      if (probe.finished) return
      probe.attempts++
      if (!wheel && wheelComponent && wheelComponent.status === Component.Error) probe.sourceError = wheelComponent.errorString()
      if (!wheel && wheelComponent && wheelComponent.status === Component.Ready) wheel = wheelComponent.createObject(probe, { controller: controller })
      if (probe.sourceError !== "") {
        probe.emitResult({ ok: false, error: probe.sourceError, launcher_found: false, missing_entry_found: false, icon_count: 0 })
        return
      }
      if (!wheel && probe.attempts > 180) {
        probe.emitResult({ ok: false, error: "production DropWheel did not instantiate", launcher_found: false, missing_entry_found: false, icon_count: 0 })
        return
      }
      if (!wheel) return
      if (wheel.screen && controller.wheelScreen !== wheel.screen) controller.wheelScreen = wheel.screen
      else if (!controller.wheelScreen && Quickshell.screens.length > 0) controller.wheelScreen = Quickshell.screens[0]
      var launcher = DesktopEntries.applications.values.find(function(entry) { return entry.id === "nvim" }) || null
      var missing = DesktopEntries.applications.values.find(function(entry) { return entry.id === "r31-no-such-entry" }) || null
      var nodes = []
      probe.descendants(wheel, nodes)
      var icons = nodes.filter(function(item) { return item.iconName !== undefined && item.resolvedSource !== undefined && item.fallbackGlyph !== undefined })
      if (probe.attempts > 180) {
        probe.emitResult({ ok: false, error: "wheel icon probe timed out", launcher_found: !!launcher, missing_entry_found: !!missing, icon_count: icons.length })
        return
      }
      if (!wheel.wheelHere || !launcher || icons.length < 4) return
      var nvim = probe.record("r31-nvim", icons)
      var override = probe.record("r31-override", icons)
      var missingDesktop = probe.record("r31-missing", icons)
      var failed = probe.record("r31-failed", icons)
      if (!nvim.found || !override.found || !missingDesktop.found || !failed.found) return
      var images = [nvim, override, missingDesktop, failed]
      if (images.some(function(item) { return item.image_status === Image.Null || item.image_status === Image.Loading })) return
      var expectedNvimSource = String(Quickshell.iconPath(launcher.icon, true) || "")
      var expectedMark = String(Qt.resolvedUrl("PLUGIN_ROOT_URL/assets/marks/herdr.svg"))
      var nvimOk = nvim.resolver_icon === String(launcher.icon) && nvim.resolver_source === expectedNvimSource && nvim.safe_icon_name === nvim.resolver_icon && nvim.safe_trusted_source === nvim.resolver_source && nvim.safe_source === expectedNvimSource && nvim.image_source === expectedNvimSource && nvim.image_ready
      var overrideOk = override.resolver_icon === "herdr" && override.resolver_source.endsWith("/assets/marks/herdr.svg") && override.safe_icon_name === "herdr" && override.safe_source === override.resolver_source && override.image_source === override.resolver_source && override.image_ready
      var missingOk = !missing && missingDesktop.resolver_icon === "herdr" && missingDesktop.resolver_source === expectedMark && missingDesktop.safe_source === expectedMark && missingDesktop.image_source === expectedMark && missingDesktop.image_ready
      var failedOk = !missing && failed.resolver_source === "file:///tmp/r31-wheel-icons-missing.svg" && failed.safe_source === failed.resolver_source && failed.image_source === failed.safe_source && failed.image_error && failed.fallback_visible && failed.fallback_text === "Z"
      probe.emitResult({ ok: nvimOk && overrideOk && missingOk && failedOk, launcher_found: true, missing_entry_found: false, expected_nvim_source: expectedNvimSource, expected_mark: expectedMark, nvim: nvim, override: override, missing_desktop: missingDesktop, failed_image: failed })
    }
  }

  IpcHandler {
    target: "fileblade.wheel-icon-probe"
    function highlight(index: int): void { controller.highlighted = index }
  }
}
QML
)
probe=${probe//PLUGIN_DROPWHEEL_URL/file:\/\/$PRODUCTION_ROOT\/ui\/DropWheel.qml}
probe=${probe//PLUGIN_ROOT_URL/file:\/\/$PRODUCTION_ROOT}
probe_data=$(printf '%s' "$probe" | base64 -w0)
guest "printf '%s' $probe_data | base64 -d > '$PROBE_PATH'"
guest "rm -f -- '$PROBE_LOG' '$PROBE_PIDFILE'; setsid quickshell -p '$PROBE_PATH' >'$PROBE_LOG' 2>&1 </dev/null & echo \$! >'$PROBE_PIDFILE'"
PROBE_PID=$(guest "cat '$PROBE_PIDFILE'" | tr -d '\r')

wait_log() {
  local marker=$1
  local deadline=$((SECONDS + ${2:-30}))
  while ((SECONDS <= deadline)); do
    if guest "grep -Fq -- $(printf '%q' "$marker") '$PROBE_LOG'"; then return 0; fi
    sleep 1
  done
  return 1
}

if ! wait_log "R31_WHEEL_ICONS_RESULT " 30; then
  diagnostic=$(guest "tail -n 30 '$PROBE_LOG'" | tr '\n' ' ' || true)
  fail harness "launch the production wheel icon probe" "no result marker; $diagnostic"
  summary
fi

result_line=$(guest "grep -F 'R31_WHEEL_ICONS_RESULT ' '$PROBE_LOG' | tail -n 1" | tr -d '\r' || true)
result_json=${result_line#*R31_WHEEL_ICONS_RESULT }
if [[ -z $result_json || $result_json == "$result_line" ]] || ! jq -e . >/dev/null 2>&1 <<<"$result_json"; then
  fail harness "parse the production wheel icon probe" "result marker did not contain JSON: $result_line"
  summary
fi

printf 'OBS R31 %s\n' "$result_json"
if ! jq -e '.ok == true' >/dev/null 2>&1 <<<"$result_json"; then
  fail harness "complete production renderer invariants" "see the OBS R31 result"
fi

json_expect() {
  local id=$1
  local label=$2
  local expression=$3
  skip "$id" && return 0
  if jq -e "$expression" >/dev/null 2>&1 <<<"$result_json"; then
    pass "$id" "$label"
  else
    local observed
    observed=$(jq -c "$expression" <<<"$result_json" 2>/dev/null || printf '%s' invalid)
    fail "$id" "$label" "observed $observed"
  fi
}

json_expect E-26-04 "production DropWheel nvim row uses the real desktop launcher icon" '.nvim.resolver_icon == .nvim.safe_icon_name and .nvim.resolver_source == .expected_nvim_source and .nvim.image_ready and .nvim.image_source == .expected_nvim_source'
json_expect E-38-03 "explicit nvim row override wins with the herdr imagery" '.override.resolver_icon == "herdr" and (.override.resolver_source | endswith("/assets/marks/herdr.svg")) and .override.image_ready and .override.image_source == .override.resolver_source'
json_expect E-38-03 "missing desktop entry falls back to the bundled herdr mark" '.missing_entry_found == false and .missing_desktop.resolver_source == .expected_mark and .missing_desktop.image_ready and .missing_desktop.image_source == .expected_mark'
json_expect E-38-03 "failed wheel image shows the resolved fallback glyph" '.failed_image.image_error and .failed_image.image_source == .failed_image.safe_source and .failed_image.fallback_visible and .failed_image.fallback_text == "Z"'

if jq -e '.nvim.descriptor_forwarded == true' >/dev/null 2>&1 <<<"$result_json"; then
  printf 'note R31 descriptor forwarding is active; this run still records the legacy image result\n'
else
  printf 'note R31 wheel supplies legacy iconName/trustedIconSource strings; failed-asset retry still requires descriptor forwarding\n'
fi

capture_shot() {
  local index=$1 name=$2 path
  guest "qs ipc -n -p '$PROBE_PATH' call -- fileblade.wheel-icon-probe highlight $index" >/dev/null
  sleep .6
  path=$("$OVM" shot "$name" 2>/dev/null | tail -n 1 || true)
  if [[ -n $path ]]; then
    printf 'shot %s  %s\n' "$name" "$path"
  else
    fail harness "$name screenshot" "the OVM shot verb returned no path"
  fi
}

capture_shot 0 E-26-04-R31-nvim
capture_shot 1 E-38-03-R31-override
capture_shot 2 E-38-03-R31-missing-desktop
capture_shot 3 E-38-03-R31-missing-image
guest "cat '$PROBE_LOG'"

summary
