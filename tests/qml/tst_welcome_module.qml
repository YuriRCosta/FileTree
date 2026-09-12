import QtQuick
import QtTest
import "../../modules/welcome" as Welcome

TestCase {
  id: test
  name: "WelcomeModule"
  width: 380
  height: 800
  QtObject {
    id: mockRegistry
    function module(id) { return ["files", "notes", "skills", "memory", "hooks", "mcp"].indexOf(id) >= 0 ? {glyph: "", iconUrl: ""} : null }
  }
  QtObject {
    id: host
    property var registry: mockRegistry
    property var placed: ({})
    property int additions: 0
    property string opened: ""
    property string focused: ""
    function findModule(id) { return placed[id] || null }
    function addSlot(edge, id, position) { placed[id] = {edge: edge, index: additions++, tab: 0}; return true }
    function setSlotTab(edge, index, tab) {}
    function setOpen(edge, value) { opened = value ? edge : "" }
    function preferredScreen(edge) { return null }
    function focusModule(id, screen, part) { focused = id }
  }
  QtObject { id: mockWelcome; property bool dismissed: false; function dismiss() { dismissed = true; return true } }
  QtObject { id: files; property var bladeHost: host; property var welcome: mockWelcome }
  QtObject {
    id: mockContext
    property int contractVersion: 3
    function service(name) { return name === "files" ? files : null }
  }
  Welcome.Module { id: view; width: test.width; height: test.height; context: mockContext }

  function init() {
    host.placed = ({welcome: {edge: "left", index: 0, tab: 0}})
    host.additions = 0
    host.opened = ""
    host.focused = ""
    mockWelcome.dismissed = false
    view.catalog = {version: 1, entries: []}
    view.error = ""
  }

  function test_core_buttons_reuse_existing_modules() {
    verify(view.openModule("skills"))
    verify(view.openModule("skills"))
    compare(host.additions, 1)
    compare(host.opened, "left")
    compare(host.focused, "skills")
    verify(!view.openModule("unknown"))
    compare(host.additions, 1)
    view.dismiss()
    verify(mockWelcome.dismissed)
  }

  function test_bad_catalog_keeps_last_local_or_valid_metadata() {
    verify(!view.acceptCatalog("offline"))
    compare(view.catalog.entries.length, 0)
    verify(view.acceptCatalog('{"version":1,"entries":[{"id":"example","name":"Example","source":"https://example.org/source","hostContract":4,"lifecycle":"on-demand","command":"never-run"}]}'))
    verify(!view.catalog.entries[0].compatible)
    verify(view.catalog.entries[0].command === undefined)
    var before = JSON.stringify(view.catalog)
    verify(!view.acceptCatalog('{"version":99,"entries":[]}'))
    compare(JSON.stringify(view.catalog), before)
    compare(view.title, "Welcome")
  }
}
