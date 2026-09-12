import QtQuick
import "../lib/PathText.js" as PathText
import Quickshell

Item {
  id: root

  required property var service

  visible: false

  property bool active: false
  property string requestId: ""
  property string mode: "open"
  property string title: "Choose a file"
  property bool multiple: false
  property var extensions: []
  property string suggestedName: ""
  property string fileName: ""
  property var results: ({})
  property int serial: 0
  property bool saveValidationBusy: false
  property string pendingSavePath: ""
  property bool pendingOverwriteApproved: false
  property bool overwriteArmed: false
  property string overwritePath: ""
  property var saveValidationResponse: null
  property string saveValidationRequestId: ""
  property int saveValidationGeneration: 0

  onFileNameChanged: {
    if (active && mode === "save") {
      clearOverwriteConfirmation()
      if (!saveValidationBusy) service.operationError = ""
    }
  }

  function normalizedExtensions(values) {
    if (!Array.isArray(values)) return []
    var result = []
    for (var index = 0; index < values.length; index++) {
      var extension = String(values[index] || "").trim().toLowerCase()
      while (extension.charAt(0) === ".") extension = extension.slice(1)
      if (extension && result.indexOf(extension) < 0) result.push(extension)
    }
    return result
  }

  function allowsEntry(path, isDir, mime) {
    if (service.chooserSession) return service.chooserSession.allowsEntry(path, isDir, mime)
    if (!active || isDir) return true
    if (mode === "folder") return false
    if (extensions.length === 0) return true
    var name = service.rootName(path).toLowerCase()
    var dot = name.lastIndexOf(".")
    var extension = dot >= 0 ? name.slice(dot + 1) : ""
    return extensions.indexOf(extension) >= 0
  }

  function optionsDocument(optionsText) {
    try {
      return optionsText && String(optionsText).trim() ? JSON.parse(String(optionsText)) : ({})
    } catch (exception) {
      return ({})
    }
  }

  function begin(optionsText) {
    var options = optionsDocument(optionsText)
    if (active && requestId) setResult(requestId, { status: "cancelled", mode: mode, paths: [] })
    serial++
    requestId = "pick-" + Date.now() + "-" + serial
    mode = ["open", "save", "folder"].indexOf(String(options.mode || "open")) >= 0 ? String(options.mode || "open") : "open"
    multiple = mode !== "save" && !!options.multiple
    extensions = normalizedExtensions(options.extensions)
    suggestedName = String(options.suggestedName || "")
    fileName = suggestedName
    title = String(options.title || (mode === "save" ? "Save a file" : (mode === "folder" ? "Choose a folder" : "Choose a file")))
    saveValidationBusy = false
    pendingSavePath = ""
    pendingOverwriteApproved = false
    clearOverwriteConfirmation()
    service.operationError = ""
    active = true
    service.clearSelection()
    service.setOpen(true)
    if (options.root) {
      var target = service.referenceScreen(null)
      service.navigateToLocation(String(options.root), target, "browse")
    }
    return requestId
  }

  function setResult(id, result) {
    var next = ({})
    var keys = Object.keys(results)
    for (var index = 0; index < keys.length; index++) next[keys[index]] = results[keys[index]]
    next[id] = result
    results = next
  }

  function clearOverwriteConfirmation() {
    overwriteArmed = false
    overwritePath = ""
  }

  function cancelValidation() {
    if (!saveValidationRequestId) return
    service.cancelBackendRequest(saveValidationRequestId, saveValidationGeneration)
    saveValidationGeneration++
    saveValidationRequestId = ""
  }

  function saveCandidate() {
    var name = fileName.trim()
    if (!name || name === "." || name === ".." || name.indexOf("/") >= 0 || name.indexOf("\u0000") >= 0)
      return { ok: false, error: "Enter a valid file name", path: "" }
    if (extensions.length > 0) {
      var dot = name.lastIndexOf(".")
      var extension = dot >= 0 ? name.slice(dot + 1).toLowerCase() : ""
      if (!extension) name += "." + extensions[0]
      else if (extensions.indexOf(extension) < 0)
        return { ok: false, error: "Use ." + extensions.join(", ."), path: "" }
    }
    return { ok: true, error: "", path: PathText.join(service.selectionDestination(), name) }
  }

  function requestValidation(path, overwriteApproved) {
    if (saveValidationRequestId) {
      service.operationError = "A destination check is already running"
      return false
    }
    pendingSavePath = String(path)
    pendingOverwriteApproved = !!overwriteApproved
    saveValidationResponse = null
    saveValidationBusy = true
    service.operationError = ""
    saveValidationGeneration++
    var requestGeneration = saveValidationGeneration
    saveValidationRequestId = service.backendRequest("save-target", ["--path", pendingSavePath], requestGeneration, function(response) {
      if (requestGeneration !== root.saveValidationGeneration) return
      root.saveValidationRequestId = ""
      root.finishValidation(response)
    })
    return false
  }

  function finishValidation(response) {
    var checkedPath = pendingSavePath
    var approved = pendingOverwriteApproved
    response = response || { ok: false, error: "Destination check failed" }
    saveValidationBusy = false
    pendingSavePath = ""
    pendingOverwriteApproved = false
    saveValidationResponse = null
    if (!active || mode !== "save") return
    var current = saveCandidate()
    if (!current.ok || current.path !== checkedPath) return
    if (!response.ok) {
      clearOverwriteConfirmation()
      service.operationError = String(response.error || "Invalid save destination")
    } else if (response.exists && !approved) {
      overwriteArmed = true
      overwritePath = checkedPath
      service.operationError = ""
    } else {
      accept([checkedPath])
    }
  }

  function accept(paths) {
    if (service.chooserSession) return service.chooserSession.accept(paths)
    var accepted = paths.slice()
    if (!multiple && accepted.length > 1) accepted = [accepted[accepted.length - 1]]
    setResult(requestId, { status: "accepted", mode: mode, paths: accepted })
    var visit = []
    for (var visited = 0; visited < accepted.length; visited++) visit.push("--path", accepted[visited])
    if (visit.length > 0) service.recordFrecencyVisit(visit)
    active = false
    requestId = ""
    saveValidationBusy = false
    cancelValidation()
    pendingSavePath = ""
    pendingOverwriteApproved = false
    clearOverwriteConfirmation()
    service.setOpen(false)
    return true
  }

  function confirm() {
    if (!active) return false
    var paths = []
    if (mode === "save") {
      var candidate = saveCandidate()
      if (!candidate.ok) {
        service.operationError = candidate.error
        return false
      }
      return requestValidation(candidate.path, overwriteArmed && overwritePath === candidate.path)
    }
    for (var index = 0; index < service.selectedEntries.length; index++) {
      var entry = service.selectedEntries[index]
      if ((mode === "folder") !== !!entry.isDir || !allowsEntry(entry.path, entry.isDir, entry.mime)) continue
      paths.push(entry.path)
    }
    if (paths.length === 0) {
      service.operationError = mode === "folder" ? "Choose a folder" : "Choose a file"
      return false
    }
    return accept(paths)
  }

  function cancel() {
    if (service.chooserSession) return service.chooserSession.cancel()
    if (!active) return
    setResult(requestId, { status: "cancelled", mode: mode, paths: [] })
    active = false
    requestId = ""
    saveValidationBusy = false
    cancelValidation()
    pendingSavePath = ""
    pendingOverwriteApproved = false
    clearOverwriteConfirmation()
    service.setOpen(false)
  }

  function activateEntry(path, isDir, name) {
    if (!active || isDir) return false
    if (!allowsEntry(path, false, "")) return true
    if (mode === "save") {
      fileName = String(name || service.rootName(path))
      return true
    }
    confirm()
    return true
  }

  function result(idValue) {
    var id = String(idValue || "")
    if (!id) return { status: "unknown", paths: [] }
    if (results[id] !== undefined) {
      var value = results[id]
      var next = ({})
      var keys = Object.keys(results)
      for (var index = 0; index < keys.length; index++) if (keys[index] !== id) next[keys[index]] = results[keys[index]]
      results = next
      return value
    }
    if (active && requestId === id)
      return { status: "pending", mode: mode, paths: [], checking: saveValidationBusy, overwriteConfirmation: overwriteArmed }
    return { status: "unknown", paths: [] }
  }

}
