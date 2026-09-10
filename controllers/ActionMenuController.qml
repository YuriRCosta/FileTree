import QtQuick
import Quickshell
import qs.Commons

Item {
  id: controller

  required property var service
  property bool open: false
  property bool standalone: false
  property string mode: "actions"
  property string path: ""
  property var entry: null
  property var paths: []
  property var entries: []
  property string input: ""
  property var screen: null
  property real menuX: Math.max(1, service.sidebarWidth - Style.space(8))
  property real menuY: Style.space(70)
  property bool rememberDefault: false
  property string placementEdge: "left"
  property bool placementKeyboard: false
  readonly property bool openLeft: placementEdge === "right"

  function visibleFor(targetScreen) {
    return open && (service.open || standalone) && (!screen || screen === targetScreen)
  }

  function show(targetMode, targetScreen, targetX, targetY, targetEntries, placement) {
    if (Array.isArray(targetEntries) && targetEntries.length > 0) assignEntries(targetEntries)
    else if (!open) captureSelection()
    var where = placement && typeof placement === "object" ? placement : ({})
    placementEdge = String(where.edge || "") === "right" ? "right" : "left"
    placementKeyboard = !!where.keyboard
    standalone = where.standalone === true
    input = ""
    mode = String(targetMode || "actions")
    service.operationError = ""
    rememberDefault = false
    if (targetScreen) screen = targetScreen
    else if (!screen) screen = service.referenceScreen(null)
    var nextX = Number(targetX)
    var nextY = Number(targetY)
    if (isFinite(nextX)) menuX = nextX
    if (isFinite(nextY)) menuY = nextY
    open = true
    if (mode === "open-with") service.loadApplications(path)
    else if (mode === "actions" && !service.clipboardReady) service.requestExternalClipboard("", false)
  }

  function fileEntry(value) {
    var raw = value && typeof value === "object" ? value : ({ path: String(value || "") })
    var target = String(raw.path || "")
    var directory = !!(raw.isDir || raw.is_dir)
    return {
      path: target,
      name: String(raw.name || service.rootName(target)),
      isDir: directory,
      isSymlink: !!(raw.isSymlink || raw.is_symlink),
      gitDeleted: false,
      gitIgnored: false,
      size: Number(raw.size === undefined ? -1 : raw.size),
      sizeText: String(raw.sizeText || raw.size_text || "—"),
      kind: String(raw.kind || (directory ? "Directory" : "File")),
      mime: String(raw.mime || (directory ? "inode/directory" : "application/octet-stream"))
    }
  }

  function assignEntries(targetEntries) {
    var normalized = []
    for (var i = 0; i < targetEntries.length; i++) {
      var candidate = fileEntry(targetEntries[i])
      if (candidate.path) normalized.push(candidate)
    }
    if (normalized.length === 0) {
      captureSelection()
      return
    }
    entries = normalized
    paths = normalized.map(function(value) { return value.path })
    path = normalized[0].path
    var first = normalized[0]
    entry = { path: first.path, name: first.name, is_dir: first.isDir, is_symlink: first.isSymlink, is_deleted: false,
      kind: first.kind, mime: first.mime, size: first.size, size_text: first.sizeText }
  }

  function captureSelection() {
    path = service.selectedPath
    entry = service.selectedMetadata ? Object.assign({}, service.selectedMetadata) : null
    paths = service.selectedPaths.slice()
    entries = service.selectedEntries.map(function(value) { return Object.assign({}, value) })
  }

  function close() {
    if (mode === "open-with") service.cancelApplicationLookup()
    open = false
    screen = null
    mode = "actions"
    path = ""
    entry = null
    paths = []
    entries = []
    input = ""
    rememberDefault = false
  }
}
