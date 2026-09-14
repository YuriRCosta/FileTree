import QtQuick
import "../modules/welcome/WelcomePlan.js" as WelcomePlan

Item {
  id: controller
  visible: false
  required property var service
  readonly property string state: String(service.welcomeState || "")
  readonly property string seenVersion: String(service.welcomeVersion || "")
  readonly property string appVersion: String(service.appVersion || "")
  readonly property bool firstRun: WelcomePlan.firstRun(state)
  readonly property bool updated: WelcomePlan.updated(state, seenVersion, appVersion)
  readonly property bool pending: WelcomePlan.pending(state, seenVersion, appVersion)
  readonly property bool needsVersionAdoption: WelcomePlan.needsVersionAdoption(state, seenVersion, appVersion)
  readonly property bool installing: false
  readonly property int installed: 0
  property string error: ""
  property bool reopened: false
  property bool versionAdopted: false
  readonly property bool ready: service.stateReady && !!service.bladeHost && service.bladeHost.layoutReady

  onReadyChanged: settle()
  Component.onCompleted: settle()

  function settle() {
    adoptVersion()
    reopenForUpdate()
  }

  function adoptVersion() {
    if (versionAdopted || !ready || !needsVersionAdoption) return false
    versionAdopted = true
    service.setWelcomeState(state, appVersion)
    return true
  }

  function removeTabs() {
    var host = service.bladeHost
    if (!host) return
    for (var attempt = 0; attempt < 64; attempt++) {
      var found = host.findModule("welcome")
      if (!found || !host.removeTab(found.edge, found.index, found.tab)) return
    }
  }

  function restoreTab(host) {
    var companion = host.findModule("notes")
    if (companion) return host.addTab(companion.edge, companion.index, "welcome", null)
    return host.addSlot("right", "welcome", -1)
  }

  function reopenForUpdate() {
    if (reopened || !updated || !ready) return false
    var host = service.bladeHost
    if (!host || !host.layoutWritable) return false
    reopened = true
    if (!host.findModule("welcome") && !restoreTab(host)) return false
    var found = host.findModule("welcome")
    if (!found) return false
    host.setSlotTab(found.edge, found.index, found.tab)
    host.setOpen(found.edge, true)
    return true
  }

  function dismiss() {
    if (!ready || !service.bladeHost.layoutWritable) return false
    service.setWelcomeState("dismissed", appVersion)
    removeTabs()
    return true
  }

  function install() {
    error = "Skills, Memory, Hooks and MCP are built in."
    return false
  }
}
