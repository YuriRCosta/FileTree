import QtQuick

Item {
  id: request

  required property var service
  property string pendingPath: ""
  property string pendingSelect: ""
  property bool pendingProperties: false

  function open(path, select, properties) {
    if (!service || !service.stateReady) return JSON.stringify({ ok: false, error: "FileBlade is still starting" })
    pendingPath = service.normalizeRoot(path)
    pendingSelect = String(select || "")
    pendingProperties = !!properties
    var navigation = service.navigateToLocation(pendingPath, service.preferredScreen(), "direct")
    service.setOpen(true)
    return JSON.stringify({ ok: true, path: pendingPath, select: pendingSelect, navigation: navigation })
  }

  Connections {
    target: request.service
    function onLocationValidationFinished(targetScreen, success, path, error, monitor) {
      if (!request.pendingPath) return
      var select = request.pendingSelect, properties = request.pendingProperties
      request.pendingPath = ""
      request.pendingSelect = ""
      request.pendingProperties = false
      if (!success) return
      if (select) {
        var known = request.service.entryForKnownPath(select)
        if (known) request.service.applySelection([known], known, known.path)
        else request.service.selectPath(select, false, request.service.rootName(select), "", "")
      }
      if (properties) request.service.focusProperties(targetScreen)
    }
  }
}
