import QtQuick
import QtTest
import "../../ui" as Ui
import "../../lib/PathText.js" as Paths
import "../../lib/KeyBindings.js" as KeyBindings

TestCase {
  id: test
  name: "UsageModules"
  width: 500
  height: 700
  visible: true
  when: windowShown
  property var module: null
  property var requests: []
  property int opened: 0

  Item {
    id: files
    property string contextPath: "/project"
    property bool autoHideSearch: false
    property var installedAgents: []
    property bool agentManagementEnabled: false
    property var artifactActions: null
    property var history: null
    property var keybindings: ({ plan: KeyBindings.compile({}) })
    function backendRequest(command, args, generation, callback) {
      requests.push({ command: command, args: args, callback: callback })
      return "request-" + requests.length
    }
    function cancelBackendRequest() {}
    function backendSubscribe() { return "watch" }
    function openDefault() { test.opened++ }
  }

  Ui.ArtifactInventory {
    id: inventory
    files: files
    providerId: "fileblade.core.mcp"
    providerRoot: "/core/mcp"
    activityMethod: "usage"
  }
  QtObject {
    id: provider
    property var inventory: inventory
    function attach(context) { inventory.attach(context) }
    function detach(context) { inventory.detach(context) }
  }
  QtObject {
    id: context
    property int contractVersion: 3
    property bool bladeOpen: true
    property bool collapsed: false
    property int tabIndex: 0
    property int cornerReserveLeft: 0
    property int cornerReserveRight: 0
    property string edge: "left"
    property var host: null
    property int slotIndex: 0
    property int tabCount: 1
    property var providerService: provider
    property var paths: Paths
    property var ui: ({ url: function(name) { return Qt.resolvedUrl("../../ui/" + name + ".qml") } })
    property var metrics: ({ options: function(rows) { return [] } })
    property var state: ({ get: function(key, fallback) { return fallback }, set: function() {} })
    function service() { return files }
    function focusNext() {}
    function focusPrevious() {}
    function closeBlade() {}
  }

  function descendant(item, name) {
    if (String(item).indexOf(name + "_") === 0) return item
    for (var child of item.children || []) {
      var found = descendant(child, name)
      if (found) return found
    }
    return null
  }

  function init() {
    requests = []; opened = 0; files.autoHideSearch = false
    inventory.activity = null; inventory.activityError = ""
  }
  function cleanup() {
    if (module) module.destroy()
    module = null
    wait(0)
  }
  function test_navigation_and_activity_lifecycle_data() {
    return [{ tag: "skills", name: "skills" }, { tag: "mcp", name: "mcp" }]
  }
  function test_navigation_and_activity_lifecycle(data) {
    var component = Qt.createComponent("../../modules/" + data.name + "/blades/Module.qml")
    compare(component.status, Component.Ready, component.errorString())
    module = component.createObject(test, { context: context, width: 385, height: 650 })
    component.destroy()
    verify(module !== null)
    inventory.activity = { ok: true, schemaVersion: 1, until: "2026-09-16", coverageStart: "2026-09-14", days: [["2026-09-14", 2, 2, 0, 0, 0]] }
    var search = descendant(module, "PaneSearchField")
    var heatmap = descendant(module, "UsageHeatmap")
    var tree = descendant(module, "ArtifactTree")
    verify(search && heatmap && tree)
    verify(inventory.activityEnabled)
    for (var autoHide of [false, true]) {
      files.autoHideSearch = autoHide
      search.reveal()
      verify(search.activeFocus)
      keyClick(Qt.Key_Tab)
      verify(heatmap.activeFocus)
      keyClick(Qt.Key_End)
      keyClick(Qt.Key_Up)
      verify(heatmap.Accessible.description.indexOf("15") >= 0)
      keyClick(Qt.Key_Backtab, Qt.ShiftModifier)
      verify(search.activeFocus)
      keyClick(Qt.Key_Tab)
      keyClick(Qt.Key_Tab)
      verify(tree.activeFocus)
    }
    inventory.activityError = "usage store unavailable"
    verify(module.status.indexOf("Activity: usage store unavailable") >= 0)
    heatmap.forceActiveFocus()
    module.view.navigationTriggered("activity")
    wait(0)
    verify(!inventory.activityEnabled)
    verify(tree.activeFocus)
    inventory.refresh(); inventory.startActivity()
    module.view.navigationTriggered("activity")
    wait(0)
    verify(inventory.activityEnabled)
    module.height = 200
    wait(0)
    verify(!inventory.activityEnabled)
    if (data.name === "mcp") {
      tree.items = [{ id: "first", name: "docs", agent: "claude", scope: "user", source: { path: "~/config", redacted: false }, observed: [{ kind: "tool", name: "search", uses: 2, failed: 1 }] }]
      tree.currentIndex = tree.firstLeafIndex()
      tree.forceActiveFocus()
      keyClick(Qt.Key_Right)
      verify(tree.expandedFolders.first)
      compare(opened, 0)
      keyClick(Qt.Key_L)
      compare(tree.rowAt(tree.currentIndex).item.name, "search")
      compare(opened, 0)
      tree.currentIndex = tree.firstLeafIndex()
      keyClick(Qt.Key_O)
      compare(opened, 1)
    }
  }
}
