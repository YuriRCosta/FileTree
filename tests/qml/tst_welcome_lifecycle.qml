import QtQuick
import QtTest
import "../../controllers" as Controllers

TestCase {
  name: "WelcomeLifecycle"

  QtObject {
    id: host
    property bool layoutReady: true
    property bool layoutWritable: true
    property var layout: ["welcome", "notes"]
    property bool refuseRemoval: false
    property int removals: 0
    function findModule(name) {
      var index = layout.indexOf(name)
      return index < 0 ? null : {edge: "right", index: 0, tab: index}
    }
    function removeTab(edge, slot, tab) {
      removals++
      if (refuseRemoval) return false
      var next = layout.slice()
      next.splice(tab, 1)
      layout = next
      return true
    }
    property bool refuseAdd: false
    property int adds: 0
    property int slotAdds: 0
    property int opens: 0
    function addTab(edge, slotIndex, moduleId, state) {
      adds++
      if (refuseAdd) return false
      layout = layout.concat([moduleId])
      return true
    }
    function addSlot(edge, moduleId, index) {
      adds++
      slotAdds++
      if (refuseAdd) return false
      layout = layout.concat([moduleId])
      return true
    }
    function setSlotTab(edge, slot, tab) { return true }
    function setOpen(edge, value, persist) { if (value) opens++; return true }
  }
  QtObject {
    id: mockService
    property bool stateReady: true
    property bool backendReady: false
    property var bladeHost: host
    property string welcomeState: ""
    property string welcomeVersion: ""
    property string appVersion: "0.2.0"
    property int requests: 0
    property int writes: 0
    function backendRequest(name, args, generation, callback) { requests++ }
    function setWelcomeState(next, version) {
      writes++
      welcomeState = next
      welcomeVersion = next === "" ? "" : String(version || "")
    }
  }
  Controllers.WelcomeController { id: welcome; service: mockService }

  function init() {
    host.layoutReady = true
    host.layoutWritable = true
    host.refuseRemoval = false
    host.removals = 0
    host.layout = ["welcome", "notes"]
    host.refuseAdd = false
    host.adds = 0
    host.slotAdds = 0
    host.opens = 0
    mockService.stateReady = true
    mockService.welcomeState = ""
    mockService.appVersion = "0.2.0"
    mockService.welcomeVersion = mockService.appVersion
    mockService.requests = 0
    mockService.writes = 0
    welcome.reopened = false
    welcome.versionAdopted = false
    welcome.error = ""
  }

  function test_dismissal_records_the_version_and_stays_closed_on_the_same_version() {
    verify(welcome.dismiss())
    compare(mockService.welcomeState, "dismissed")
    compare(mockService.welcomeVersion, "0.2.0")
    verify(!welcome.updated)
    verify(!welcome.pending)
    welcome.reopened = false
    verify(!welcome.reopenForUpdate())
    compare(host.adds, 0)
    compare(host.opens, 0)
    compare(host.layout, ["notes"])
  }

  function test_a_legacy_dismissal_adopts_the_running_version_without_reopening() {
    mockService.welcomeState = "dismissed"
    mockService.welcomeVersion = ""
    welcome.reopened = false
    welcome.settle()
    compare(mockService.welcomeVersion, "0.2.0")
    compare(mockService.writes, 1)
    verify(!welcome.updated)
    verify(!welcome.pending)
    compare(host.adds, 0)
    compare(host.layout, ["welcome", "notes"])
  }

  function test_a_new_version_reopens_welcome_once_for_its_release_notes() {
    verify(welcome.dismiss())
    compare(host.layout, ["notes"])
    mockService.appVersion = "0.3.0"
    welcome.reopened = false
    verify(welcome.updated)
    verify(welcome.pending)
    verify(welcome.reopenForUpdate())
    compare(host.layout, ["notes", "welcome"])
    compare(host.adds, 1)
    compare(host.slotAdds, 0)
    compare(host.opens, 1)
    verify(!welcome.reopenForUpdate())
    compare(host.adds, 1)
    verify(welcome.dismiss())
    compare(mockService.welcomeVersion, "0.3.0")
    verify(!welcome.updated)
    compare(host.layout, ["notes"])
  }

  function test_reopen_for_update_needs_a_writable_layout() {
    verify(welcome.dismiss())
    mockService.appVersion = "0.3.0"
    welcome.reopened = false
    host.layoutWritable = false
    verify(!welcome.reopenForUpdate())
    compare(host.adds, 0)
    compare(host.layout, ["notes"])
    host.layoutWritable = true
    host.refuseAdd = true
    verify(!welcome.reopenForUpdate())
    compare(host.adds, 1)
    compare(host.layout, ["notes"])
  }

  function test_reopen_happens_when_the_layout_becomes_ready() {
    verify(welcome.dismiss())
    host.layoutReady = false
    mockService.appVersion = "0.3.0"
    welcome.reopened = false
    verify(!welcome.reopenForUpdate())
    compare(host.adds, 0)
    host.layoutReady = true
    wait(0)
    compare(host.adds, 1)
    compare(host.opens, 1)
    compare(host.layout, ["notes", "welcome"])
  }

  function test_dismiss_is_explicit_and_preserves_other_tabs() {
    verify(welcome.pending)
    compare(host.layout, ["welcome", "notes"])
    verify(welcome.dismiss())
    compare(mockService.welcomeState, "dismissed")
    compare(mockService.writes, 1)
    compare(host.layout, ["notes"])
    verify(!welcome.pending)
  }

  function test_reopen_survives_late_layout_and_state_hydration_data() {
    return [{tag: "dismissed", state: "dismissed"}, {tag: "legacy-installed", state: "installed"}]
  }
  function test_reopen_survives_late_layout_and_state_hydration(data) {
    mockService.stateReady = false
    host.layoutReady = false
    mockService.welcomeState = data.state
    host.layout = ["notes", "welcome"]
    mockService.stateReady = true
    host.layoutReady = true
    wait(0)
    compare(host.layout, ["notes", "welcome"])
    compare(mockService.welcomeState, data.state)
    compare(mockService.writes, 0)
    verify(!welcome.pending)
    verify(welcome.dismiss())
    compare(host.layout, ["notes"])
    host.layout = ["notes", "welcome"]
    wait(0)
    compare(host.layout, ["notes", "welcome"])
    compare(mockService.welcomeState, "dismissed")
  }

  function test_dismiss_waits_for_ready_writable_layout() {
    for (var property of ["layoutReady", "layoutWritable"]) {
      host[property] = false
      verify(!welcome.dismiss())
      host[property] = true
    }
    mockService.stateReady = false
    verify(!welcome.dismiss())
    compare(host.layout, ["welcome", "notes"])
    compare(mockService.writes, 0)
    compare(host.removals, 0)
  }

  function test_no_installer_requests_or_polling_offline() {
    mockService.stateReady = false
    mockService.stateReady = true
    verify(!welcome.install())
    verify(welcome.error.indexOf("built in") >= 0)
    wait(1100)
    compare(mockService.requests, 0)
    compare(mockService.writes, 0)
    compare(host.layout, ["welcome", "notes"])
    verify(!welcome.installing)
    compare(welcome.installed, 0)
  }

  function test_refused_removal_is_bounded() {
    host.refuseRemoval = true
    welcome.dismiss()
    compare(host.removals, 1)
    compare(host.layout, ["welcome", "notes"])
  }
}
