import QtQuick
import Quickshell
import qs.Commons

Scope {
  id: session
  required property var manager
  required property var offer
  readonly property string handle: offer.handle
  readonly property var service: browser.item
  readonly property alias chooserWindow: window
  readonly property var backend: ChooserBackend { owner: session.manager.service; session: session }
  readonly property var filters: {
    var values = offer.filters.slice()
    if (offer.current_filter && !values.some(function(value) { return JSON.stringify(value) === JSON.stringify(offer.current_filter) })) values.push(offer.current_filter)
    return values
  }
  property int filterIndex: Math.max(0, filters.findIndex(function(value) { return JSON.stringify(value) === JSON.stringify(offer.current_filter) }))
  readonly property var filter: filters[filterIndex] || null
  property var allowedPaths: ({})
  property bool opened: false
  property bool finished: false
  property bool busy: false
  property string commandId: ""

  onFilterChanged: {
    allowedPaths = ({})
    if (service) { service.clearSelection(); service.refreshTree() }
  }

  Loader {
    id: browser
    Component.onCompleted: setSource(Util.fileUrl(session.manager.service.pluginDir + "/app/chooser/ChooserBrowser.qml"), {
      shell: session.manager.service.shell,
      manifest: session.manager.service.manifest,
      pluginRegistry: null,
      chooserSession: session
    })
    onLoaded: Qt.callLater(session.begin)
  }

  function begin() {
    if (!service || finished) return
    service.beginPicker(JSON.stringify({ mode: offer.mode, title: offer.title, multiple: offer.multiple, root: offer.current_folder || service.home, suggestedName: offer.current_name }))
    Qt.callLater(function() { session.focus(offer.mode === "save" ? "name" : "tree") })
  }

  function setOpen(value) {
    if (!value) { cancel(); return }
    manager.service.bladeHost.yieldFocus()
    opened = true
  }

  function focus(part) { window.focusPart(part) }

  function allowsEntry(path, isDir, mime) {
    if (isDir) return true
    if (offer.mode === "folder") return false
    return !filter || allowedPaths[path] === true
  }

  function confirm() {
    if (!service || busy || finished) return false
    if (offer.mode !== "save") return service.pickerControllerApi.confirm()
    var candidate = service.pickerControllerApi.saveCandidate()
    if (!candidate.ok) { service.operationError = candidate.error; return false }
    return accept([candidate.path])
  }

  function accept(paths) {
    if (busy || finished) return false
    var argv = ["choose", "--handle", handle]
    for (var path of paths) argv.push("--path", path)
    if (filter) argv.push("--filter", JSON.stringify(filter))
    if (service.pickerOverwriteArmed) argv.push("--overwrite")
    busy = true
    commandId = manager.service.backendRequest("chooser", argv, 0, function(response) {
      session.commandId = ""
      session.busy = false
      if (session.finished) return
      if (!response || !response.ok) { session.service.operationError = String(response && response.error || "Chooser response failed"); return }
      if (response.decision.status === "overwrite") {
        session.service.pickerOverwriteArmed = true
        session.service.pickerOverwritePath = response.decision.path
        session.service.operationError = "Confirm replacement of this file"
      } else session.close()
    })
    return false
  }

  function cancel() {
    if (finished) return
    finished = true
    opened = false
    if (commandId) manager.service.cancelBackendRequest(commandId, 0, true)
    manager.service.backendRequest("chooser", ["cancel", "--handle", handle], 0, function() {})
  }

  function close() {
    finished = true
    opened = false
  }

  ChooserWindow { id: window; session: session }

  Component.onDestruction: {
    if (commandId) manager.service.cancelBackendRequest(commandId, 0, true)
  }
}
