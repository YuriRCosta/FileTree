import QtQuick
import "../modules/welcome/WelcomePlan.js" as WelcomePlan

Item {
  id: controller
  visible: false
  required property var service
  readonly property string state: String(service.welcomeState || "")
  readonly property bool pending: WelcomePlan.pending(state)
  readonly property bool installing: false
  readonly property int installed: 0
  property string error: ""
  readonly property bool ready: service.stateReady && !!service.bladeHost && service.bladeHost.layoutReady

  function removeTabs() {
    var host = service.bladeHost
    if (!host) return
    for (var attempt = 0; attempt < 64; attempt++) {
      var found = host.findModule("welcome")
      if (!found || !host.removeTab(found.edge, found.index, found.tab)) return
    }
  }

  function dismiss() {
    if (!ready || !service.bladeHost.layoutWritable) return false
    service.setWelcomeState("dismissed")
    removeTabs()
    return true
  }

  function install() {
    error = "Skills, Memory, Hooks and MCP are built in."
    return false
  }
}
