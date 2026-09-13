import QtQuick

Item {
  id: controller

  required property var service
  property alias model: drivesModel
  property alias allModel: allVolumesModel
  property bool running: false
  property bool actionsAvailable: false
  property string error: ""
  property string busySource: ""
  property int generation: 0
  property string requestId: ""
  property int failureCount: 0
  property bool showSystemVolumes: false
  property var volumeRows: []
  property var peerLocations: ({})
  property var peerCandidates: ({})
  property var savedLocations: []
  property string locationsRequestId: ""
  property int locationsGeneration: 0
  property string peerRequestId: ""
  property int peerRequestSerial: 0

  signal connectionRequested(string locationId, string label, string host, string user, string path)
  signal locationRequested(var descriptor, var targetScreen)

  ListModel { id: drivesModel }
  ListModel { id: allVolumesModel }

  readonly property int count: drivesModel.count
  readonly property int volumeCount: allVolumesModel.count
  readonly property var tierOrder: ["external", "tailnet", "unmounted", "internal", "system"]

  function tierLabel(tier) {
    if (tier === "tailnet") return "Tailnet"
    if (tier === "external") return "External"
    if (tier === "unmounted") return "Not mounted"
    if (tier === "internal") return "Internal"
    if (tier === "system") return "System"
    return String(tier || "")
  }

  function tierRank(tier) {
    var rank = tierOrder.indexOf(String(tier || ""))
    return rank < 0 ? tierOrder.length : rank
  }

  function restartDelay() {
    return Math.min(30000, 500 * Math.pow(2, Math.min(failureCount, 6)))
  }

  function visibleTier(tier) {
    if (tier === "external" || tier === "unmounted") return true
    return showSystemVolumes
  }

  function glyphFor(volume) {
    if (peerLocations[String(volume.source)]) return "󰒍"
    if (volume.filesystem === "SFTP") return "󰒋"
    if (volume.image) return "󰗮"
    if (volume.bus === "usb") return "󱊞"
    if (volume.removable) return "󰑹"
    return "󰋊"
  }

  function actionFor(row) {
    var peer = peerLocations[String(row.source)]
    if (peer) {
      var disconnect = !!peer.session_generation
      var verb = disconnect ? "Disconnect" : "Connect"
      return { command: disconnect ? "location-disconnect" : "location-connect", glyph: disconnect ? "󰇪" : "󰄠", tip: verb + " " + row.name, actions: [{ button: "left", text: verb }], context: [] }
    }
    if (!row.mounted) {
      var context = []
      if (row.readOnly) context.push({ text: "mounts read-only" })
      if (row.needsAuthorization) context.push({ text: "asks for authentication" })
      return { command: "mount-volume", glyph: "󰄠", tip: "Mount " + row.name, actions: [{ button: "left", text: "Mount" }], context: context }
    }
    if (row.external) return { command: "eject-volume", glyph: "󰇪", tip: "Eject " + row.name, actions: [{ button: "left", text: "Eject" }], context: [] }
    return { command: "unmount-volume", glyph: "󰇪", tip: "Unmount " + row.name, actions: [{ button: "left", text: "Unmount" }], context: [] }
  }

  function rowFor(volume) {
    var size = Number(volume.size) || 0
    var used = volume.used === null || volume.used === undefined ? -1 : Number(volume.used)
    return {
      name: String(volume.name || volume.device || ""),
      device: String(volume.device || ""),
      source: String(volume.source || ""),
      mountpoint: String(volume.mountpoint || ""),
      mounted: !!volume.mounted,
      filesystem: String(volume.filesystem || ""),
      label: String(volume.label || ""),
      bus: String(volume.bus || ""),
      image: String(volume.image || ""),
      size: size,
      sizeLabel: String(volume.size_label || ""),
      used: used,
      available: Number(volume.available) || 0,
      usedFraction: size > 0 && used >= 0 ? used / size : -1,
      removable: !!volume.removable,
      external: !!volume.external,
      readOnly: !!volume.read_only,
      needsAuthorization: !!volume.needs_authorization,
      tier: String(volume.tier || ""),
      volumeGlyph: glyphFor(volume)
    }
  }

  function apply(payload) {
    if (!payload || !Array.isArray(payload.volumes)) return
    actionsAvailable = !!payload.actions
    volumeRows = payload.volumes.map(rowFor)
    rebuild()
  }

  function rebuild() {
    var rows = volumeRows.slice()
    for (var id in peerLocations) {
      var peer = peerLocations[id]
      var connected = peer.connection === "connected" && (peer.capabilities || []).indexOf("list") >= 0
      rows.push(rowFor({ name: peer.label, source: id, mountpoint: peer.canonical_uri, mounted: connected,
        filesystem: "SFTP", tier: "tailnet", size_label: connected ? "" : (peer.session_generation ? "Disconnect needed" : "Not connected") }))
    }
    rows.sort(function(left, right) {
      var byTier = tierRank(left.tier) - tierRank(right.tier)
      return byTier !== 0 ? byTier : left.name.localeCompare(right.name)
    })
    drivesModel.clear()
    allVolumesModel.clear()
    for (var rowIndex = 0; rowIndex < rows.length; rowIndex++) {
      allVolumesModel.append(rows[rowIndex])
      var row = rows[rowIndex]
      if (row.tier === "tailnet" || visibleTier(row.tier)) drivesModel.append(row)
    }
  }

  function applyLocations(payload) {
    var peers = {}
    var candidates = {}
    if (payload && payload.ok) {
      var locations = payload.locations || []
      for (var index = 0; index < locations.length; index++)
        if (locations[index].kind === "sftp") peers[locations[index].id] = locations[index]
      var discovered = payload.tailnet && payload.tailnet.candidates || []
      for (var candidate = 0; candidate < discovered.length; candidate++) candidates[discovered[candidate].id] = discovered[candidate]
    }
    peerLocations = peers
    peerCandidates = candidates
    savedLocations = payload && payload.ok && payload.saved || []
    rebuild()
  }

  function cancelLocations() {
    locationsGeneration++
    if (locationsRequestId) service.cancelBackendRequest(locationsRequestId, locationsGeneration - 1)
    locationsRequestId = ""
  }

  function refreshLocations() {
    if (!service.backendReady || locationsRequestId || peerRequestId) return
    var serial = ++locationsGeneration
    locationsRequestId = service.backendRequest("locations", [], serial, function(response) {
      if (serial !== controller.locationsGeneration) return
      controller.locationsRequestId = ""
      controller.applyLocations(response)
      var failure = String(response && response.ok
        ? (response.tailnet && response.tailnet.error || response.saved_error || "")
        : (response && response.error || "Locations unavailable"))
      if (failure) controller.error = failure
    })
  }

  function requestPeerConnection(id) {
    if (busySource) return
    var peer = peerCandidates[id]
    if (!peer) return
    var saved = savedLocations.filter(function(value) { return value.host === peer.host })[0]
    connectionRequested(id, String(peer.label), String(peer.host), saved ? String(saved.user) : "", saved ? String(saved.path) : "/")
  }

  function connectPeer(id, host, user, path, save) {
    var peer = peerLocations[id]
    if (!peer || peer.session_generation || !peerCandidates[id]) return
    var arguments = ["--location", id, "--expected-host", String(host), "--user", String(user), "--path", String(path)]
    if (save) arguments.push("--save")
    peerAction("location-connect", id, arguments)
  }

  function peerAction(command, id, arguments) {
    if (busySource || !service.backendReady) return
    cancelLocations()
    busySource = id
    error = ""
    var serial = ++peerRequestSerial
    peerRequestId = service.backendRequest(command, arguments, serial, function(response) {
      if (serial !== controller.peerRequestSerial) return
      controller.peerRequestId = ""
      controller.busySource = ""
      controller.error = String(response && response.ok ? (response.save_error || "") : (response && response.error || command + " failed"))
      var peers = Object.assign({}, controller.peerLocations)
      if (response && response.location && typeof response.location === "object") peers[id] = response.location
      else delete peers[id]
      controller.peerLocations = peers
      controller.rebuild()
      controller.refreshLocations()
    }, null, 200000, { untimed: true })
  }

  function cancelPeerAction() {
    if (peerRequestId) service.cancelBackendRequest(peerRequestId, peerRequestSerial)
  }

  function actionAvailableFor(source) { return !!peerLocations[String(source)] || actionsAvailable }

  function descriptorForPath(path) {
    var matches = Object.keys(peerLocations).map(function(id) { return controller.peerLocations[id] }).filter(function(peer) {
      var base = String(peer.canonical_uri).replace(/\/$/, "")
      return peer.connection === "connected" && (peer.capabilities || []).indexOf("list") >= 0
        && (path === base || path === peer.canonical_uri || String(path).indexOf(base + "/") === 0)
    })
    return matches.sort(function(a, b) { return b.canonical_uri.length - a.canonical_uri.length })[0] || null
  }

  function relativePath(peer, path) {
    var base = String(peer.canonical_uri).replace(/\/$/, "")
    if (path === base || path === peer.canonical_uri) return "."
    if (String(path).indexOf(base + "/") !== 0) return null
    try {
      var parts = String(path).slice(base.length + 1).split("/").map(decodeURIComponent)
      if (parts.some(function(part) { return part === ".." || part.indexOf("/") >= 0 || part.indexOf("\0") >= 0 })) return null
      return parts.join("/")
    } catch (_) { return null }
  }

  function invalidatePeer(id, generation) {
    var peers = Object.assign({}, peerLocations)
    if (!peers[id] || peers[id].session_generation !== generation) return
    delete peers[id]
    peerLocations = peers
    rebuild()
  }

  function start() {
    if (!service.backendReady || requestId) return
    generation++
    var requestGeneration = generation
    running = true
    requestId = service.backendSubscribeTopic("mounts", [], requestGeneration, function(event) {
      if (requestGeneration !== controller.generation) return
      controller.apply(event.payload)
    }, function(response) {
      if (requestGeneration !== controller.generation) return
      controller.running = true
      controller.failureCount = 0
      controller.error = ""
      controller.apply(response.payload)
    }, function(response) {
      if (requestGeneration !== controller.generation) return
      controller.requestId = ""
      controller.running = false
      var failed = !(response && (response.ok || response.cancelled))
      if (!failed) return
      controller.failureCount++
      controller.error = String(response && response.error || "drive watcher stopped")
      console.warn("data-goblin.fileblade: drive watcher failed (" + controller.failureCount + "): " + controller.error)
      restartTimer.interval = controller.restartDelay()
      restartTimer.restart()
    })
  }

  function stop() {
    if (!requestId) return
    service.cancelBackendRequest(requestId, generation)
    generation++
    requestId = ""
    running = false
  }

  function indexOfSource(source) {
    for (var index = 0; index < allVolumesModel.count; index++)
      if (String(allVolumesModel.get(index).source) === String(source)) return index
    return -1
  }

  function clearError() { error = "" }

  function act(command, source) {
    if (busySource !== "" || !actionsAvailable) return
    busySource = String(source)
    error = ""
    service.backendRequest(command, ["--source", String(source)], generation, function(response) {
      controller.busySource = ""
      if (response && response.ok) return
      controller.error = String(response && response.error || command + " failed")
    }, null, 200000, { untimed: true })
  }

  function mountVolume(source) { act("mount-volume", source) }
  function unmountVolume(source) { act("unmount-volume", source) }
  function ejectVolume(source) { act("eject-volume", source) }

  function runAction(source) {
    var index = indexOfSource(source)
    if (index < 0) return
    var peer = peerLocations[String(source)]
    if (peer) {
      if (peer.session_generation) peerAction("location-disconnect", String(source), ["--location", String(source), "--generation", String(peer.session_generation)])
      else requestPeerConnection(String(source))
      return
    }
    act(actionFor(allVolumesModel.get(index)).command, source)
  }

  function openVolume(source, targetScreen) {
    if (busySource === String(source)) return
    var index = indexOfSource(source)
    if (index < 0) return
    var peer = peerLocations[String(source)]
    if (peer) {
      if (peer.connection === "connected" && (peer.capabilities || []).indexOf("list") >= 0) locationRequested(peer, targetScreen)
      else runAction(source)
      return
    }
    var row = allVolumesModel.get(index)
    if (!row.mounted) {
      mountVolume(source)
      return
    }
    service.navigateToLocation(String(row.mountpoint), targetScreen, "favorite")
  }

  Timer {
    id: restartTimer
    interval: 500
    onTriggered: controller.start()
  }

  Connections {
    target: controller.service
    function onDrivesModeChanged() { if (controller.service.drivesMode) controller.refreshLocations() }
    function onBackendReadyChanged() {
      if (controller.service.backendReady) {
        controller.start()
        if (controller.service.drivesMode) controller.refreshLocations()
      }
      else {
        controller.requestId = ""
        controller.running = false
        controller.cancelLocations()
        controller.peerRequestSerial++
        controller.peerRequestId = ""
        controller.busySource = ""
        controller.applyLocations(null)
      }
    }
  }

  Component.onCompleted: start()
}
