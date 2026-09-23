import QtQuick

Item {
  id: controller
  required property var service
  property bool ready: false
  property bool saving: false
  property string error: ""
  property var settings: ({})
  property string writtenBy: ""
  property bool newerWriter: false
  property int pendingTrashDays: 0
  property int generation: 0
  property string requestId: ""
  readonly property bool trashAnswered: ready && typeof settings.trashRetentionDays === "number"
  readonly property int trashRetentionDays: trashAnswered ? settings.trashRetentionDays : 0
  readonly property bool trashCleanupConsent: trashAnswered && trashRetentionDays > 0

  function showPendingConsent() {
    if (!ready || trashAnswered || !service.bladeHost || !service.bladeHost.layoutReady) return
    var host = service.bladeHost
    if (host.pendingOpenEdges) return
    var edge = host.side || "left"
    if (!host.bladeFor(edge).open) host.setOpen(edge, true)
  }
  onTrashAnsweredChanged: Qt.callLater(showPendingConsent)
  onReadyChanged: Qt.callLater(showPendingConsent)
  Connections {
    target: controller.service.bladeHost
    function onLayoutReadyChanged() { controller.showPendingConsent() }
    function onPendingOpenEdgesChanged() { Qt.callLater(controller.showPendingConsent) }
    function onLayoutRevisionChanged() { Qt.callLater(controller.showPendingConsent) }
  }

  function read() {
    if (!service.backendReady || saving) return
    ready = false
    if (requestId) service.cancelBackendRequest(requestId, generation, true)
    var current = ++generation
    requestId = service.backendRequest("preferences-read", [], current, function(response) {
      if (current !== controller.generation) return
      controller.requestId = ""
      controller.receive(response)
    })
  }

  function receive(response) {
    if (!response || response.ok !== true || !response.settings || typeof response.settings.trashRetentionDays !== "number")
      pendingTrashDays = 0
    if (!response || response.ok !== true || !response.settings) {
      settings = ({})
      error = String(response && response.error || "Preferences could not be read")
      ready = true
      return false
    }
    settings = response.settings
    writtenBy = String(response.writtenBy || "")
    newerWriter = response.newerWriter === true
    ready = true
    error = ""
    return true
  }

  function change(arguments) {
    if (!ready || saving) return false
    saving = true
    var current = ++generation
    if (requestId) service.cancelBackendRequest(requestId, current - 1, true)
    requestId = service.backendRequest("preferences-set", arguments, current, function(response) {
      if (current !== controller.generation) return
      controller.requestId = ""
      controller.saving = false
      controller.receive(response)
    })
    return true
  }

  function setTrashRetentionDays(days, consent) {
    if (!isFinite(Number(days)) || days < 0 || days > 3650 || (days > 0 && consent !== true)) return false
    return change(["--trash-retention-days", String(Math.round(days))])
  }

  function refreshSoon() { debounce.restart() }
  Timer { id: debounce; interval: 150; onTriggered: controller.read() }
  Connections {
    target: controller.service
    function onBackendReadyChanged() { if (controller.service.backendReady) controller.read() }
  }
  Component.onCompleted: read()
  Component.onDestruction: {
    if (requestId) service.cancelBackendRequest(requestId, generation, true)
  }
}
