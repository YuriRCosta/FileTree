import QtQuick
import Quickshell.Io

Item {
  id: probe
  required property var manager

  function geometry(handle, matches) {
    var session = manager.sessions[handle]
    if (!session) return "{}"
    var queue = [session.chooserWindow.contentItem]
    while (queue.length) {
      var item = queue.shift()
      if (item.visible && matches(item)) {
        var point = item.mapToItem(null, 0, 0)
        return JSON.stringify({ x: point.x, y: point.y, width: item.width, height: item.height })
      }
      if (item.children) for (var child of item.children) queue.push(child)
    }
    return "{}"
  }

  IpcHandler {
    target: "fileblade.chooser"
    function status(): string {
      var sessions = []
      for (var handle of Object.keys(probe.manager.sessions)) {
        var item = probe.manager.sessions[handle], service = item.service
        var rows = []
        if (service) for (var i = 0; i < service.treeModel.count; i++) {
          var row = service.treeModel.get(i)
          rows.push({ path: row.path, name: row.name, isDir: row.isDir, selectable: item.allowsEntry(row.path, row.isDir, row.mime) })
        }
        sessions.push({ handle: handle, title: item.offer.title || "Choose a file — FileBlade", opened: item.opened, root: service ? service.rootPath : "", selected: service ? service.selectedEntries : [], rows: rows, error: service ? service.operationError : "", overwrite: service ? service.pickerOverwriteArmed : false })
      }
      return JSON.stringify({ sessions: sessions, error: probe.manager.error })
    }
    function fixture(document: string): string {
      if (probe.manager.transportEnabled) return "disable transport before using an isolated UI fixture"
      probe.manager.present(JSON.parse(document).offers)
      return "presented"
    }
    function rowGeometry(handle: string, path: string): string {
      return probe.geometry(handle, function(item) { return item.path === path && item.selectable !== undefined })
    }
    function buttonGeometry(handle: string, label: string): string {
      return probe.geometry(handle, function(item) { return item.text === label && item.down !== undefined })
    }
  }
}
