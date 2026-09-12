import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons

Item {
  id: root
  required property var service
  required property var barConfig
  property bool barHidden: false
  readonly property int barSize: barConfig.position === "left" || barConfig.position === "right"
    ? Style.bar.sizeVertical : Style.bar.sizeHorizontal
  readonly property string toggles: Quickshell.env("HOME") + "/.local/state/omarchy/toggles"
  property string watchId: ""
  property int generation: 0
  property bool probeAgain: false

  function refresh() {
    if (probe.running) probeAgain = true
    else probe.running = true
  }

  function subscribe() {
    if (!service || !service.backendReady) return
    var previous = generation++
    if (watchId) service.cancelBackendRequest(watchId, previous, true)
    var current = generation
    watchId = service.backendSubscribe([toggles], current,
      function(event) { if (current === root.generation) root.refresh() },
      function(response) { if (current === root.generation) root.refresh() },
      function(response) {
        if (current !== root.generation) return
        root.watchId = ""
        retry.restart()
      })
  }

  onServiceChanged: Qt.callLater(subscribe)
  Connections {
    target: root.service
    function onBackendReadyChanged() { if (root.service.backendReady) root.subscribe() }
  }
  Timer { id: retry; interval: 1000; onTriggered: root.subscribe() }
  Process {
    id: probe
    running: true
    command: ["sh", "-c", "if [ -f \"$1\" ]; then echo yes; else echo no; fi", "fileblade-bar", root.toggles + "/bar-off"]
    stdout: SplitParser { onRead: function(line) { root.barHidden = String(line).trim() === "yes" } }
    onExited: {
      if (root.probeAgain) {
        root.probeAgain = false
        running = true
      }
    }
  }
  Component.onDestruction: {
    if (service && watchId) service.cancelBackendRequest(watchId, generation, true)
  }
}
