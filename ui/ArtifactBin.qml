import QtQuick
import "../lib/PathText.js" as PathText
import "../lib/ArtifactBinRows.js" as ArtifactBinRows

Item {
  id: bin

  property string module: ""
  required property var service
  property var context: null
  ActionKeyGuard { id: binKeys; active: bin.active; shared: bin.context && bin.context.hostWindow ? bin.context.hostWindow.actionKeys : null }
  property var describe: function(entry) { return null }
  property var helperRoute: null
  property var removalArguments: function(entry) { return [] }
  property bool active: true
  property alias rows: listing.rows
  readonly property var actions: service ? service.artifactActions : null
  readonly property var actionState: actions ? actions.stateFor(module) : ({})
  readonly property bool busy: actionState.busy === true
  readonly property string error: localError || String(actionState.error || "")
  property string localError: ""
  property var pending: null
  property string pendingMode: ""
  property string actionRequestId: ""
  property var restoring: ({})
  readonly property alias choices: dialog.options

  signal changed()

  function isBinned(entry) {
    return !!entry && String(entry.kind || "") === "bin"
  }

  function rowAction(entry) {
    if (!entry) return null
    if (isBinned(entry)) return { glyph: "󰑖", title: "Restore" }
    return describe(entry) ? { glyph: "󰩺", title: "Delete", danger: true } : null
  }

  function refuse(message) {
    pending = null
    pendingMode = ""
    localError = String(message)
    bin.changed()
  }

  function trashablePaths(entry) {
    if (!entry || isBinned(entry)) return []
    var item = describe(entry)
    var paths = item && Array.isArray(item.paths) ? item.paths : []
    return paths.filter(function(path) { return String(path || "") !== "" })
  }

  function groupFor(entry) {
    if (!isBinned(entry)) return null
    return Array.isArray(entry.groups) && entry.groups.length > 0 ? entry.groups : ["Trash"]
  }

  function sourceOf(entry) { return ArtifactBinRows.sourceOf(entry) }

  function mergeRows(live, cached, groupsFor) { return ArtifactBinRows.merge(live, cached, restoring, groupsFor) }

  function rememberRestore(entry) {
    var id = sourceOf(entry)
    if (id === "") return
    var next = {}
    for (var key in restoring) next[key] = restoring[key]
    var position = Number(entry.position)
    next[id] = { row: ArtifactBinRows.placeholderFor(entry), position: isFinite(position) && position >= 0 ? Math.floor(position) : 0 }
    restoring = next
    restoreExpiry.restart()
  }

  function stateSuffix(entry) {
    // Decided not to show the bin date in the interface; the record keeps deletedAt.
    // return isBinned(entry) && entry.deletedAt ? "  " + String(entry.deletedAt) : ""
    return ""
  }

  function entryPath(entry) {
    if (!entry) return ""
    if (entry.path) return String(entry.path)
    return entry.source && entry.source.path ? String(entry.source.path) : ""
  }

  function targetPath(entry, describedPath) {
    var source = entry && entry.source && typeof entry.source === "object" ? entry.source : ({})
    var target = String(entry && (entry.realpath || entry.linkTarget || entry.link_target)
      || source.realpath || source.linkTarget || source.link_target || "")
    var logical = entryPath(entry)
    var described = String(describedPath || logical)
    if (!target || target === logical) return described
    if (logical === described) return target
    if (logical !== described && PathText.within(logical, described)) {
      var suffix = PathText.fileUrl(logical).slice(PathText.fileUrl(described).length)
      var targetUrl = PathText.fileUrl(target)
      if (targetUrl.slice(-suffix.length) === suffix) return PathText.fromFileUrl(targetUrl.slice(0, -suffix.length))
    }
    return target
  }

  function ask(entry) {
    if (!entry || busy) return
    localError = ""
    pending = entry
    var item = isBinned(entry) ? null : describe(entry)
    var shownPath = item && item.path ? targetPath(entry, item.path) : entryPath(entry)
    var heading = String(entry.name || "") + "\n" + shownPath
    if (isBinned(entry)) {
      dialog.open(heading,
                  [{ key: "cancel", label: "Cancel" }, { key: "purge", label: "Delete forever", danger: true }, { key: "restore", label: "Restore" }])
      return
    }
    var options = [{ key: "cancel", label: "Cancel" }, { key: "bin", label: "Disable" }]
    if (trashablePaths(entry).length > 0) options.push({ key: "trash", label: "Trash", danger: true })
    dialog.open(heading, options)
  }

  function run(command, mode) {
    if (!command || command.length === 0) return
    if (String(command[0]) !== service.cliPath || String(command[1]) !== "_backend" || !command[2]) {
      localError = "Artifact actions must use the FileBlade backend"
      settle()
      return
    }
    pendingMode = mode
    localError = ""
    actionRequestId = actions ? actions.run(module, String(command[2]), command.slice(3)) : ""
    if (!actionRequestId) { localError = "Recovery is busy or unavailable"; settle() }
  }

  function choose(key) {
    var entry = pending
    if (!entry) return
    var id = String(entry.id || "")
    if (key === "restore") {
      run([service.cliPath, "_backend", "bin-restore", "--module", module, "--id", id].concat(actions ? actions.restoreArguments(module) : []), "restore")
    } else if (key === "purge") {
      run([service.cliPath, "_backend", "bin-purge", "--module", module, "--id", id], "purge")
    } else if (key === "trash") {
      var paths = trashablePaths(entry)
      if (paths.length === 0) {
        refuse("This item lives inside a shared configuration file, so there is no file to trash")
        return
      }
      var command = service.backendCommand("trash")
      command.push("--follow-symlinks")
      for (var i = 0; i < paths.length; i++) command.push("--path", String(paths[i]))
      run(command, "trash")
    } else if (key === "bin") {
      var item = describe(entry)
      if (!item) {
        refuse("This item can no longer be changed; refresh and retry")
        return
      }
      var realpath = targetPath(entry, item.path)
      if (realpath && realpath !== String(item.path || "")) item.realpath = realpath
      var command = [service.cliPath, "_backend", helperRoute ? "bin-remove" : "bin-put", "--module", module, "--item", JSON.stringify(item)]
      if (helperRoute) command.push("--helper-route", JSON.stringify(helperRoute), "--arguments", JSON.stringify(removalArguments(entry)))
      run(command, "bin")
    }
  }

  function settle() {
    if (pendingMode === "restore" && pending && !localError) rememberRestore(pending)
    actionRequestId = ""
    pending = null
    pendingMode = ""
    refresh()
    bin.changed()
  }
  function syncRegistration() {
    if (actions && helperRoute) actions.registerRestore(module, helperRoute)
  }

  function refresh() {
    listing.refresh()
  }

  onModuleChanged: syncRegistration()
  onHelperRouteChanged: syncRegistration()
  onActionsChanged: syncRegistration()
  Component.onCompleted: syncRegistration()

  Connections {
    target: bin.actions
    function onFinished(module, requestId, response) {
      if (module !== bin.module) return
      bin.refresh()
      if (requestId === bin.actionRequestId) bin.settle()
    }
  }

  Timer { id: restoreExpiry; interval: 5000; onTriggered: bin.restoring = ({}) }

  ArtifactBinListing {
    id: listing
    service: bin.service
    module: bin.module
    active: bin.active
  }

  ActionDialog {
    id: dialog
    actionKeys: binKeys
    anchors.fill: parent
    onChosen: function(key) { bin.choose(key) }
    onCanceled: bin.pending = null
  }
}
