import QtQuick
import "../lib/PathText.js" as PathText

Item {
  id: controller

  required property var service
  property var operations: null
  property var trash: null
  property var drives: null
  property var attached: []
  readonly property bool demanded: attached.length > 0
  readonly property bool shown: demanded && status === "ready"
  readonly property var tip: describe(status, usedLabel, sizeLabel, percent, availableLabel, mountpoint, filesystem)
  readonly property string rootPath: String(service.rootPath || "")
  readonly property bool eligible: rootPath !== "" && !service.trashMode && !service.drivesMode && !service.recentMode && !PathText.isRemote(rootPath)
  property string status: "hidden"
  property string path: ""
  property string mountpoint: ""
  property string filesystem: ""
  property string source: ""
  property real size: -1
  property real used: -1
  property real available: -1
  property real fraction: -1
  property int percent: -1
  property string sizeLabel: ""
  property string usedLabel: ""
  property string availableLabel: ""
  property string error: ""
  property string requestId: ""
  property int generation: 0
  property int requestGeneration: 0
  property int failures: 0
  property int requestCount: 0
  readonly property int periodicInterval: 60000
  readonly property int debounceInterval: 400
  readonly property int deadlineMs: 5000

  function describe(state, used, size, percentValue, free, mount, kind) {
    if (state !== "ready") return { title: "", context: [], text: "" }
    var title = used + " of " + size + " used"
    var full = percentValue + "% full"
    var left = free + " free"
    var context = [{ glyph: "\u{f02ca}", text: full }, { glyph: "\u{f19c}", text: left }]
    if (mount) context.push({ glyph: "\u{f024b}", text: mount + (kind ? "  " + kind : "") })
    var text = title + ", " + full + ", " + left + (mount ? " on " + mount + (kind ? " (" + kind + ")" : "") : "")
    return { title: title, context: context, text: text }
  }

  function attach(pane) {
    if (!pane || attached.indexOf(pane) >= 0) return
    attached = attached.concat([pane])
  }

  function detach(pane) {
    var index = attached.indexOf(pane)
    if (index < 0) return
    attached = attached.slice(0, index).concat(attached.slice(index + 1))
  }

  function clear() {
    path = ""
    mountpoint = ""
    filesystem = ""
    source = ""
    size = -1
    used = -1
    available = -1
    fraction = -1
    percent = -1
    sizeLabel = ""
    usedLabel = ""
    availableLabel = ""
    error = ""
  }

  function cancelPending() {
    if (!requestId) return
    service.cancelBackendRequest(requestId, requestGeneration, true)
    requestId = ""
  }

  function settle() {
    if (!eligible || !demanded) status = "hidden"
    else if (fraction >= 0) status = "ready"
    else if (error !== "") status = "unavailable"
    else status = "loading"
  }

  function reset() {
    generation++
    cancelPending()
    failures = 0
    retryTimer.stop()
    clear()
    settle()
    schedule()
  }

  function schedule() {
    if (!eligible || !demanded) {
      settle()
      return
    }
    debounce.restart()
  }

  function request() {
    if (!eligible || !demanded || !service.backendReady || requestId) return
    var root = rootPath
    var sent = generation
    requestGeneration = sent
    requestCount++
    requestId = service.backendRequest("capacity", ["--path", root], sent, function(response) {
      if (sent !== controller.generation || root !== controller.rootPath) return
      controller.requestId = ""
      controller.apply(response)
    }, null, deadlineMs)
  }

  function number(value) {
    return value === null || value === undefined ? -1 : Number(value)
  }

  function apply(response) {
    if (response && response.ok === true) {
      path = String(response.path || "")
      mountpoint = String(response.mountpoint || "")
      filesystem = String(response.filesystem || "")
      source = String(response.source || "")
      size = number(response.size)
      used = number(response.used)
      available = number(response.available)
      fraction = response.fraction === null || response.fraction === undefined ? -1 : Number(response.fraction)
      percent = response.percent === null || response.percent === undefined ? -1 : Math.round(Number(response.percent))
      sizeLabel = String(response.size_label || "")
      usedLabel = String(response.used_label || "")
      availableLabel = String(response.available_label || "")
      error = fraction >= 0 ? "" : "capacity unavailable"
      if (fraction >= 0) failures = 0
    } else {
      clear()
      error = String(response && response.error || "capacity request failed")
    }
    settle()
    if (status === "unavailable") {
      failures++
      retryTimer.interval = Math.min(30000, 1000 * Math.pow(2, Math.max(0, failures - 1)))
      retryTimer.restart()
    }
  }

  onEligibleChanged: schedule()
  onDemandedChanged: schedule()

  Timer {
    id: debounce
    interval: controller.debounceInterval
    onTriggered: controller.request()
  }

  Timer {
    id: retryTimer
    interval: 1000
    onTriggered: if (controller.status === "unavailable") controller.request()
  }

  Timer {
    id: periodic
    interval: controller.periodicInterval
    repeat: true
    running: controller.demanded && controller.eligible && controller.status !== "loading"
    onTriggered: controller.request()
  }

  Connections {
    target: controller.service
    function onRootPathChanged() { controller.reset() }
    function onBackendReadyChanged() { if (controller.service.backendReady) controller.schedule() }
    function onTreeRefreshRequested() { controller.schedule() }
  }

  Connections {
    target: controller.operations
    ignoreUnknownSignals: true
    function onOperationCompleted() { controller.schedule() }
  }

  Connections {
    target: controller.trash
    ignoreUnknownSignals: true
    function onOperationFinished() { controller.schedule() }
  }

  Connections {
    target: controller.drives
    ignoreUnknownSignals: true
    function onVolumeRowsChanged() { controller.schedule() }
  }
}
