import QtQuick
import "MediaModel.js" as MediaModel

Item {
  id: provider

  required property var controller
  property bool active: false
  property bool recursive: false
  property string rootPath: controller.rootPath
  property bool showHidden: controller.showHidden
  property var descriptor: null
  property var rows: []
  property bool busy: false
  property string error: ""
  property bool limited: false
  property int generation: 0
  property string requestId: ""
  property var requestOwner: null
  property var queue: []
  property int visited: 0
  property int scanned: 0
  property int maximumEntries: 100000
  property int maximumDirectories: 4096
  property bool fresh: false
  readonly property int pageSize: 400
  readonly property bool readable: MediaModel.allows(descriptor, "list") && MediaModel.allows(descriptor, "read")

  function cancel() {
    var old = generation
    generation++
    if (requestId && requestOwner) requestOwner.cancelBackendRequest(requestId, old, true)
    requestId = ""
    requestOwner = null
    queue = []
    busy = false
    pump.stop()
  }

  function reload(invalidate) {
    cancel()
    busy = active
    rows = []
    error = ""
    limited = false
    visited = 0
    scanned = 0
    fresh = invalidate === true
    if (!active) return
    if (!MediaModel.allows(descriptor, "list") || !MediaModel.allows(descriptor, "read")) { error = "Media is unavailable for this location"; busy = false; return }
    if (rootPath === "" || rootPath.charAt(0) !== "/") { error = "No local media location"; busy = false; return }
    queue = [{ path: rootPath, start: 0 }]
    busy = true
    pump.start()
  }

  function nextPage() {
    if (!active || !busy || requestId) return
    if (queue.length === 0) { busy = false; return }
    if (scanned >= maximumEntries) { limited = true; busy = false; queue = []; return }
    var page = queue[0]
    queue = queue.slice(1)
    var args = ["--path", page.path, "--start", String(page.start), "--count", String(pageSize), "--sort", "name"]
    if (showHidden) args.push("--show-hidden")
    if (fresh && page.start === 0) args.push("--fresh")
    if (!controller.gitEnabled) args.push("--no-git")
    var mine = generation
    requestOwner = controller
    requestId = controller.backendRequest("children-window", args, mine, function(response) {
      if (mine !== provider.generation || !provider.active) return
      provider.requestId = ""
      if (!response || !response.ok) {
        provider.error = String(response && response.error || "Unable to read media")
        pump.start()
        return
      }
      var entries = Array.isArray(response.entries) ? response.entries : []
      var added = []
      entries.forEach(function(entry) {
        provider.scanned++
        var row = provider.controller.makeRow(entry, 1)
        row.relative = String(row.path).slice(provider.rootPath === "/" ? 1 : provider.rootPath.length + 1)
        if (MediaModel.kind(row)) added.push(MediaModel.record(row))
        if (provider.recursive && row.isDir && !row.isSymlink) {
          if (provider.visited < provider.maximumDirectories) {
            provider.visited++
            provider.queue.push({ path: row.path, start: 0 })
          } else provider.limited = true
        }
      })
      provider.rows = provider.rows.concat(added)
      if (response.capped) provider.limited = true
      if (response.truncated) provider.queue.unshift({ path: page.path, start: Number(response.start) + provider.pageSize })
      pump.start()
    })
  }

  onActiveChanged: reload(false)
  onRootPathChanged: if (active) reload(false)
  onRecursiveChanged: if (active) reload(false)
  onShowHiddenChanged: if (active) reload(false)
  onDescriptorChanged: if (active) reload(false)
  Component.onDestruction: cancel()

  Connections {
    target: provider.controller
    ignoreUnknownSignals: true
    function onBackendReadyChanged() { if (provider.active && provider.controller.backendReady) provider.reload(false) }
  }

  Timer { id: pump; interval: 0; onTriggered: provider.nextPage() }
}
