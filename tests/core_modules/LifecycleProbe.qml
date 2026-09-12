import QtQuick
import Quickshell.Io

Item {
  id: probe
  property var service: null
  property var backend: null
  property var originalLayout: null
  property string originalRoot: ""

  function snapshot() {
    var result = { pending: Object.keys(backend.pending), modules: {} }
    for (var name of ["skills", "memory", "hooks", "mcp"]) {
      var provider = service.services["fileblade.core." + name]
      var inventory = provider ? provider.inventory : null
      result.modules[name] = {
        views: provider ? provider.viewCount : -1,
        ready: !!inventory && inventory.ready,
        busy: !!inventory && inventory.busy,
        error: inventory ? inventory.loadError || inventory.watchError : "",
        rows: inventory ? inventory.items : [],
        lanes: inventory ? inventory.lanes.map(function(lane) {
          return { scan: lane.scan ? lane.scan.id : "", watch: lane.watch ? lane.watch.id : "" }
        }) : []
      }
    }
    return result
  }

  IpcHandler {
    target: "fileblade.core-lifecycle"
    function status(): string { return JSON.stringify(probe.snapshot()) }
    function begin(project: string): string {
      probe.originalLayout = JSON.parse(JSON.stringify(probe.service.bladeHost.layout))
      probe.originalRoot = probe.service.rootPath
      probe.service.bladeHost.setOpen("left", false)
      probe.service.bladeHost.setOpen("right", false)
      probe.service.setRootPath(project, false)
      return "ready"
    }
    function show(module: string): string {
      var host = probe.service.bladeHost
      host.setSlots("left", [host.newSlot("files")])
      host.setSlots("right", [host.newSlot(module)])
      host.setOpen("right", true)
      return "shown"
    }
    function close(): string {
      probe.service.bladeHost.setOpen("right", false)
      return "closed"
    }
    function restore(): string {
      probe.service.setRootPath(probe.originalRoot, false)
      probe.service.bladeHost.replaceLayout(probe.originalLayout)
      return "restored"
    }
  }
}
