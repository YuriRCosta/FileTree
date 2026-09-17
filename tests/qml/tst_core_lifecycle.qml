import QtQuick
import QtTest

TestCase {
  id: test
  name: "CoreLifecycle"
  property var provider: null
  property var requests: []
  property var watches: []
  property var cancelled: []
  Item { id: first }
  Item { id: second }
  Item {
    id: files
    property string contextPath: "/original"
    property bool agentManagementEnabled: true
    function backendRequest(command, args, generation, callback) {
      var id = "read-" + requests.length
      requests.push({ id: id, command: command, args: args.slice(), callback: callback })
      return id
    }
    function backendSubscribe(paths, generation, event, ready, closed) {
      var id = "watch-" + watches.length
      watches.push({ id: id, event: event, ready: ready, closed: closed })
      return id
    }
    function cancelBackendRequest(id) { cancelled.push(id) }
  }

  function init() {
    requests = []; watches = []; cancelled = []
    files.contextPath = "/original"
  }
  function cleanup() {
    if (provider) provider.destroy()
    provider = null
    wait(0)
  }
  function test_lifecycle_data() {
    return ["skills", "memory", "hooks", "mcp"].map(function(name) { return { tag: name, module: name } })
  }
  function reply(index, name) {
    var response = { ok: true, schemaVersion: 1, healthBasis: "configuration-only", watchPaths: ["/original/" + index] }
    response[provider.inventory.itemsKey] = [{ name: name }]
    requests[index].callback(response)
  }
  function test_lifecycle(data) {
    var component = Qt.createComponent("../../modules/" + data.module + "/Provider.qml")
    compare(component.status, Component.Ready, component.errorString())
    provider = component.createObject(test, {
      files: files, providerId: "fileblade.core." + data.module, providerRoot: "/core/" + data.module,
      inventoryComponentUrl: Qt.resolvedUrl("../../ui/ArtifactInventory.qml")
    })
    component.destroy()
    verify(provider !== null)
    compare(provider.inventory, null)
    wait(220)
    compare(requests.length, 0)
    compare(watches.length, 0)
    verify(provider.attach(first))
    verify(provider.attach(first))
    verify(provider.attach(second))
    compare(provider.viewCount, 2)
    var inventory = provider.inventory
    compare(inventory.maximumItems, { skills: 256, memory: 1000, hooks: 1000, mcp: 1024 }[data.module])
    compare(inventory.itemsKey, data.module === "mcp" ? "definitions" : "items")
    compare(inventory.activityMethod, ["skills", "mcp"].indexOf(data.module) >= 0 ? "usage" : "")
    inventory.startScan()
    compare(requests.length, 2)
    reply(0, "project"); reply(1, "user")
    compare(watches.length, 2)
    provider.detach(first)
    compare(cancelled.length, 0)
    provider.detach(second)
    compare(cancelled, ["watch-0", "watch-1"])
    verify(!inventory.ready)
    for (var watch of watches) {
      watch.event({ events: ["create"] })
      watch.ready({ skipped: [] })
      watch.closed({ error: "late callback" })
    }
    inventory.startScan()
    wait(220)
    compare(requests.length, 2)
    verify(inventory.lanes.every(function(lane) { return !lane.watch && !lane.scan && !lane.busy }))
    provider.attach(first)
    inventory.startScan()
    compare(requests.length, 4)
    provider.detach(first)
    verify(cancelled.indexOf("read-2") >= 0 && cancelled.indexOf("read-3") >= 0)
    reply(2, "stale"); reply(3, "stale")
    verify(inventory.items.every(function(row) { return row.name !== "stale" }))
    provider.attach(first)
    inventory.startScan()
    reply(4, "project"); reply(5, "user")
    var args = ["--project", "/original", "--id", "accepted"]
    verify(inventory.mutate("apply", args))
    args[1] = "/changed-selection"
    provider.detach(first)
    files.contextPath = "/next-project"
    verify(inventory.applying)
    compare(inventory.mutation.project, "/original")
    compare(JSON.parse(requests[6].args[9]), ["--project", "/original", "--id", "accepted"])
    verify(cancelled.indexOf("read-6") < 0)
    requests[6].callback({ ok: true, schemaVersion: 1 })
    verify(!inventory.applying)
    wait(220)
    compare(requests.length, 7)
    provider.attach(second)
    inventory.startScan()
    compare(JSON.parse(requests[7].args[9])[1], "/next-project")
    provider.detach(second)
    reply(7, "late"); reply(8, "late")
    provider.shutdown()
    compare(provider.inventory, null)
    compare(provider.viewCount, 0)
    verify(!provider.attach(first))
  }
}
