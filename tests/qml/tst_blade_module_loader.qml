import QtQuick
import QtTest
import "../../blades"

TestCase {
  id: test
  name: "BladeModuleContextLifetime"
  property var activity: []
  property var loaded: null
  property int writes: 0
  property int focuses: 0
  property int writtenTab: -1

  QtObject {
    id: first
    function attach(context) { test.activity.push("attach:" + context.providerId) }
    function detach(context) { test.activity.push("detach:" + context.providerId) }
  }
  QtObject {
    id: second
    function attach(context) { test.activity.push("attach-second:" + context.providerId) }
    function detach(context) { test.activity.push("detach-second:" + context.providerId) }
  }
  QtObject {
    id: dirs
    function stateDir(module) { return "/state/" + module }
    function configDir(module) { return "/config/" + module }
    function ready(module) { return true }
  }
  QtObject {
    id: host
    property var dirs: dirs
    property int moduleContractVersion: 2
    property string pluginDir: ""
    function findSlotId(edge, id) { return id === "slot" ? 0 : -1 }
    function slotTabs(edge, index) {
      return [{module:"one", state:{label:"first"}}, {module:"two", state:{label:"second"}}]
    }
    function slotModuleAt(edge, index, tab) { return slotTabs(edge, index)[tab].module }
    function setTabStateValue(edge, index, tab, key, value) { test.writes++; test.writtenTab = tab; return true }
    function focusBlade(edge, screen, index, part, toggle) { test.focuses++ }
    function focusRelativeSlot(edge, index, direction, screen) { test.focuses++ }
    function reportSlotFocus(edge, index, focused) { test.focuses++ }
  }
  BladeContext {
    id: live
    host: host
    services: ({first:first, second:second})
    slotId: "slot"
    slotIndex: 0
    bladeOpen: true
  }
  Component { id: loaderComponent; BladeModuleLoader { liveContext: live } }

  function init() {
    activity = []; writes = 0; focuses = 0; writtenTab = -1
    live.moduleId = "one"; live.moduleDir = "/plugins/one"; live.providerId = "first"
    live.tabIndex = 0; live.bladeOpen = true
    live.definition = { settings: { schema: [{ key: "label", type: "string", maxLength: 64, defaultValue: "default" }] } }
    loaded = createTemporaryObject(loaderComponent, test)
    loaded.loadModule(Qt.resolvedUrl("ModuleContextProbe.qml"))
    verify(loaded.item !== null)
  }
  function cleanup() {
    if (loaded) { loaded.loadModule(""); wait(0); loaded.destroy(); loaded = null }
  }

  function test_settings_schema_survives_context_creation() {
    compare(loaded.moduleContext.settings.schema.length, 1)
    verify(loaded.moduleContext.settings.has("label"))
    compare(loaded.moduleContext.settings.get("label"), "first")
  }

  function test_incoming_identity_does_not_rebind_the_outgoing_module() {
    var old = loaded.item
    live.moduleId = "two"; live.moduleDir = "/plugins/two"; live.providerId = "second"; live.tabIndex = 1
    compare(old.context.moduleId, "one")
    compare(old.context.moduleDir, "/plugins/one")
    compare(old.context.providerId, "first")
    compare(old.provider, first)
    compare(old.context.state.get("label", ""), "first")
    live.bladeOpen = false
    verify(!old.context.bladeOpen)
  }

  function test_retiring_context_stops_view_work_state_writes_and_focus_requests() {
    var context = loaded.moduleContext
    verify(context.state.set("active", true))
    context.requestFocus(""); compare(focuses, 1)
    loaded.retire()
    verify(context.retired); verify(!context.bladeOpen)
    verify(!context.state.set("stale", true)); compare(writes, 1)
    context.requestFocus(""); context.focusNext(); context.focusPrevious(); context.reportFocus(false)
    compare(focuses, 1)
  }

  function test_same_entry_reload_creates_new_context_and_tears_down_against_original_provider() {
    var previous = loaded.moduleContext
    live.moduleId = "two"; live.providerId = "second"; live.tabIndex = 1
    loaded.loadModule(Qt.resolvedUrl("ModuleContextProbe.qml"))
    verify(loaded.moduleContext !== previous)
    compare(loaded.item.provider, second)
    compare(loaded.moduleContext.state.get("label", ""), "second")
    wait(0)
    compare(activity, ["attach:first", "detach:first", "attach-second:second"])
    compare(writes, 0); compare(focuses, 0)
  }

  function test_retirement_flushes_the_original_tab_before_refusing_late_writes() {
    loaded.item.flushOnClose = true
    live.moduleId = "two"; live.providerId = "second"; live.tabIndex = 1
    loaded.retire()
    compare(writes, 1); compare(writtenTab, 0)
    verify(!loaded.moduleContext.state.set("late", true))
    compare(writes, 1)
  }

  function test_unload_and_owner_destruction_detach_the_original_provider() {
    loaded.loadModule(""); wait(0)
    compare(activity, ["attach:first", "detach:first"])
    compare(loaded.moduleContext, null)
    compare(writes, 0); compare(focuses, 0)
    loaded.loadModule(Qt.resolvedUrl("ModuleContextProbe.qml"))
    loaded.destroy(); wait(0); loaded = null
    compare(activity, ["attach:first", "detach:first", "attach:first", "detach:first"])
  }
}
