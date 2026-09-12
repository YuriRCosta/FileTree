import QtQuick
import "../lib/PathText.js" as PathText

Item {
  id: controller

  required property var service
  property bool busy: false
  property string path: ""
  property string error: ""
  property string activePath: ""
  property string activeMode: ""
  property string activeOrigin: ""
  property string pendingPath: ""
  property string pendingMode: ""
  property var activeTargetScreen: null
  property string activeMonitor: ""
  property var pendingTargetScreen: null
  property var activeResponse: null
  property int validationCount: 0
  property int cancellationCount: 0
  property int historyPruneCount: 0
  property string recoveryOrigin: ""
  property string recoveryNotice: ""
  property string lastMissingRoot: ""
  property string lastRecoveredRoot: ""
  property int recoveryCount: 0
  property string activeRequestId: ""
  property int generation: 0

  function clearError() {
    error = ""
  }

  function normalizedMode(value) {
    var mode = String(value || "direct")
    return ["direct", "browse", "favorite", "back", "forward"].indexOf(mode) >= 0 ? mode : "direct"
  }

  function start(targetPath, targetScreen, mode) {
    var target = service.normalizeRoot(targetPath)
    activePath = target
    activeMode = normalizedMode(mode)
    activeOrigin = service.normalizeRoot(service.rootPath)
    activeTargetScreen = targetScreen || null
    activeMonitor = service.bladeHost.focusedMonitorName
    activeResponse = null
    path = target
    error = ""
    busy = true
    validationCount++
    generation++
    var requestGeneration = generation
    activeRequestId = service.backendRequest("stat-batch", ["--path", target], requestGeneration, function(response) {
      if (requestGeneration !== controller.generation) return
      controller.activeRequestId = ""
      controller.activeResponse = response
      controller.finish(0)
    })
  }

  function navigate(targetPath, targetScreen, mode) {
    var target = service.normalizeRoot(targetPath)
    var navigationMode = normalizedMode(mode)
    if (PathText.isRemote(target) && [service.trashResource, service.recentResource, service.drivesResource].indexOf(target) < 0) {
      cancel(false)
      busy = true
      path = target
      var serial = ++generation
      activeRequestId = service.backendRequest("locations", [], serial, function(response) {
        if (serial !== controller.generation) return
        controller.activeRequestId = ""
        controller.busy = false
        service.drivesController.applyLocations(response)
        controller.navigateDescriptor(service.drivesController.descriptorForPath(target), targetScreen, navigationMode, target)
      })
      return "checking"
    }
    path = target
    error = ""
    if (target === service.normalizeRoot(service.rootPath) && service.treeModel.count > 0) {
      cancel(false)
      if (navigationMode === "direct") service.locationValidationFinished(targetScreen || null, true, service.rootPath, "", service.bladeHost.focusedMonitorName)
      else if (targetScreen) service.focusTree(targetScreen)
      return "current"
    }
    if (target === service.trashResource || target === service.recentResource || target === service.drivesResource) return openVirtual(target, targetScreen, navigationMode)
    if (activeRequestId) {
      queue(target, targetScreen, navigationMode)
      return "checking"
    }
    start(target, targetScreen, navigationMode)
    return "checking"
  }

  function navigateDescriptor(descriptor, targetScreen, mode, targetPath) {
    var target = String(targetPath || descriptor && descriptor.canonical_uri || "")
    var relative = descriptor ? service.drivesController.relativePath(descriptor, target) : null
    if (!descriptor || !descriptor.session_generation || relative === null) {
      error = "Remote location is unavailable; reconnect it in Drives"
      if (mode === "back" || mode === "forward") handleHistoryFailure({ mode: mode, path: target, origin: service.normalizeRoot(service.rootPath) })
      return "unavailable"
    }
    cancel(false)
    activePath = target
    activeMode = normalizedMode(mode)
    activeOrigin = service.normalizeRoot(service.rootPath)
    activeTargetScreen = targetScreen || null
    activeMonitor = service.bladeHost.focusedMonitorName
    path = target
    error = ""
    busy = true
    var serial = ++generation
    activeRequestId = service.backendRequest("list", ["--location", descriptor.id, "--generation", descriptor.session_generation,
      "--path", relative, "--count", "1", "--no-git"], serial, function(response) {
      if (serial !== controller.generation) return
      controller.activeRequestId = ""
      controller.busy = false
      var request = controller.takeRequest()
      if (response && response.ok) controller.handleSuccess(request, { path: target, is_dir: true })
      else {
        if (response && response.error_id === "stale-location") service.drivesController.invalidatePeer(descriptor.id, descriptor.session_generation)
        controller.handleFailure(request, String(response && response.error || "Remote directory unavailable"))
      }
    })
    return "checking"
  }

  Connections {
    target: controller.service.drivesController || null
    function onLocationRequested(descriptor, targetScreen) { controller.navigateDescriptor(descriptor, targetScreen, "browse") }
  }

  function openVirtual(target, targetScreen, mode) {
    var origin = service.normalizeRoot(service.rootPath)
    cancel(false)
    path = target
    error = ""
    if (mode === "back" || mode === "forward") {
      if (!commitHistoryDestination(mode, target, origin)) {
        error = "Location history changed while opening " + target
        return "history-changed"
      }
    } else {
      service.setRootPath(target)
    }
    if (mode === "direct") service.locationValidationFinished(targetScreen || null, true, target, "", service.bladeHost.focusedMonitorName)
    else if (targetScreen) service.focusTree(targetScreen)
    return "opened"
  }

  function queue(target, targetScreen, mode) {
    pendingPath = target
    pendingMode = mode
    pendingTargetScreen = targetScreen || null
    busy = true
    cancellationCount++
    service.cancelBackendRequest(activeRequestId, generation)
    generation++
    activeRequestId = ""
    pendingPath = ""
    pendingMode = ""
    pendingTargetScreen = null
    start(target, targetScreen, mode)
  }

  function cancel(clearValidationError) {
    pendingPath = ""
    pendingMode = ""
    pendingTargetScreen = null
    clearActive()
    busy = false
    if (clearValidationError !== false) error = ""
    if (activeRequestId) {
      cancellationCount++
      service.cancelBackendRequest(activeRequestId, generation)
      generation++
      activeRequestId = ""
    }
  }

  function clearActive() {
    activePath = ""
    activeMode = ""
    activeOrigin = ""
    activeTargetScreen = null
    activeResponse = null
  }

  function discardUnavailableHistoryDestination(mode, targetPath, origin) {
    if (service.normalizeRoot(service.rootPath) !== service.normalizeRoot(origin)) return false
    var target = service.normalizeRoot(targetPath)
    var history = mode === "back" ? service.rootBackStack.slice() : service.rootForwardStack.slice()
    if (history.length === 0 || service.normalizeRoot(history[history.length - 1]) !== target) return false
    history.pop()
    if (mode === "back") service.rootBackStack = history
    else service.rootForwardStack = history
    service.scheduleStateSave()
    return true
  }

  function discardCurrentHistoryDestinations(mode) {
    var current = service.normalizeRoot(service.rootPath)
    var history = mode === "back" ? service.rootBackStack.slice() : service.rootForwardStack.slice()
    var removed = 0
    while (history.length > 0 && service.normalizeRoot(history[history.length - 1]) === current) {
      history.pop()
      removed++
    }
    if (removed === 0) return 0
    if (mode === "back") service.rootBackStack = history
    else service.rootForwardStack = history
    historyPruneCount += removed
    service.scheduleStateSave()
    return removed
  }

  function commitHistoryDestination(mode, targetPath, origin) {
    var previous = service.normalizeRoot(service.rootPath)
    if (previous !== service.normalizeRoot(origin)) return false
    var destination = service.normalizeRoot(targetPath)
    var source = mode === "back" ? service.rootBackStack.slice() : service.rootForwardStack.slice()
    if (source.length === 0 || service.normalizeRoot(source[source.length - 1]) !== destination) return false
    source.pop()
    var opposite = mode === "back" ? service.rootForwardStack.slice() : service.rootBackStack.slice()
    if (opposite.length === 0 || service.normalizeRoot(opposite[opposite.length - 1]) !== previous) opposite.push(previous)
    if (opposite.length > 50) opposite = opposite.slice(opposite.length - 50)
    applyHistory(mode, source, opposite)
    service.setRootPath(destination, false, true)
    return true
  }

  function applyHistory(mode, source, opposite) {
    if (mode === "back") {
      service.rootBackStack = source
      service.rootForwardStack = opposite
    } else {
      service.rootForwardStack = source
      service.rootBackStack = opposite
    }
  }

  function finish(exitCode) {
    var request = takeRequest()
    if (request.nextPath) {
      Qt.callLater(function() { controller.start(request.nextPath, request.nextScreen, request.nextMode) })
      return
    }
    if (!request.path) {
      busy = false
      return
    }
    var validation = validate(request.response, request.path, exitCode)
    busy = false
    if (validation.error) {
      handleFailure(request, validation.error)
      return
    }
    handleSuccess(request, validation.entry)
  }

  function takeRequest() {
    var request = {
      path: activePath,
      mode: normalizedMode(activeMode),
      origin: activeOrigin,
      screen: activeTargetScreen,
      monitor: activeMonitor,
      response: activeResponse,
      nextPath: pendingPath,
      nextMode: pendingMode,
      nextScreen: pendingTargetScreen
    }
    clearActive()
    pendingPath = ""
    pendingMode = ""
    pendingTargetScreen = null
    return request
  }

  function validate(response, targetPath, exitCode) {
    var result = response || { ok: false, error: "Location backend exited with " + exitCode }
    var entries = result && Array.isArray(result.entries) ? result.entries : []
    var entry = entries.length > 0 ? entries[0] : null
    if (!result.ok || !entry) return { error: String(result.error || "Folder does not exist"), entry: null }
    if (!entry.is_dir) return { error: "Not a folder: " + targetPath, entry: entry }
    return { error: "", entry: entry }
  }

  function handleFailure(request, validationError) {
    if (request.mode === "back" || request.mode === "forward") {
      handleHistoryFailure(request)
      return
    }
    error = request.mode === "favorite"
      ? "Favorite is unavailable: " + request.path + " — " + validationError
      : validationError
    if (request.mode === "direct") service.locationValidationFinished(request.screen, false, request.path, validationError, String(request.monitor || ""))
  }

  function handleHistoryFailure(request) {
    if (!discardUnavailableHistoryDestination(request.mode, request.path, request.origin)) {
      error = "Folder history changed while checking " + request.path
      return
    }
    discardCurrentHistoryDestinations(request.mode)
    var canContinue = request.mode === "back" ? service.canGoBack : service.canGoForward
    if (!canContinue) {
      error = "No available " + request.mode + " folder — skipped " + request.path
      return
    }
    busy = true
    Qt.callLater(function() {
      if (request.mode === "back") service.goBack(request.screen)
      else service.goForward(request.screen)
    })
  }

  function handleSuccess(request, entry) {
    var destination = service.normalizeRoot(entry.path || request.path)
    path = destination
    error = ""
    if (request.mode === "back" || request.mode === "forward") {
      if (!commitHistoryDestination(request.mode, destination, request.origin)) {
        error = "Folder history changed while checking " + destination
        return
      }
    } else {
      service.setRootPath(destination)
    }
    var late = String(request.monitor || "") !== service.bladeHost.focusedMonitorName
    if (request.mode === "direct") service.locationValidationFinished(request.screen, true, destination, "", String(request.monitor || ""))
    else if (request.screen && !late) service.focusTree(request.screen)
  }

  function restartRecoveryNotice() { recoveryNoticeTimer.restart() }
  function stopRecoveryNotice() { recoveryNoticeTimer.stop() }

  Timer {
    id: recoveryNoticeTimer
    interval: 6000
    onTriggered: controller.recoveryNotice = ""
  }

}
