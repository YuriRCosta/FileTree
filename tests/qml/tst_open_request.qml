import QtQuick
import QtTest
import "../../app" as App

TestCase {
  id: testCase
  name: "OpenRequest"

  property var navigations: []
  property var selections: []
  property var focused: []

  QtObject {
    id: fakeService
    property bool stateReady: true
    property bool open: false
    signal locationValidationFinished(var targetScreen, bool success, string path, string error, string monitor)
    function normalizeRoot(path) { return String(path).replace(/\/+$/, "") }
    function preferredScreen() { return "screen-1" }
    function rootName(path) { return String(path).split("/").pop() }
    function setOpen(value) { open = !!value }
    function entryForKnownPath(path) { return path === "/known/dir/file" ? { path: path, name: "file" } : null }
    function applySelection(entries, primary, anchor) { testCase.selections = testCase.selections.concat([{ known: true, path: anchor }]) }
    function selectPath(path, isDir, name) { testCase.selections = testCase.selections.concat([{ known: false, path: path, name: name }]) }
    function focusProperties(screen) { testCase.focused = testCase.focused.concat([screen]) }
    function navigateToLocation(path, screen, mode) {
      testCase.navigations = testCase.navigations.concat([{ path: path, screen: screen, mode: mode }])
      return "checking"
    }
  }

  App.OpenRequest { id: subject; service: fakeService }

  function init() {
    navigations = []
    selections = []
    focused = []
    fakeService.open = false
  }

  function test_open_navigates_directly_shows_the_window_and_selects_after_validation() {
    var response = JSON.parse(subject.open("/known/dir/", "/known/dir/file", false))
    compare(response.ok, true)
    compare(response.path, "/known/dir")
    compare(navigations, [{ path: "/known/dir", screen: "screen-1", mode: "direct" }])
    compare(fakeService.open, true)
    compare(selections, [])
    fakeService.locationValidationFinished("screen-1", true, "/known/dir", "", "")
    compare(selections, [{ known: true, path: "/known/dir/file" }])
    compare(focused, [])
    fakeService.locationValidationFinished("screen-1", true, "/known/dir", "", "")
    compare(selections.length, 1)
  }

  function test_unlisted_selection_and_properties_focus() {
    subject.open("/other", "/other/new.txt", true)
    fakeService.locationValidationFinished("screen-1", true, "/other", "", "")
    compare(selections, [{ known: false, path: "/other/new.txt", name: "new.txt" }])
    compare(focused, ["screen-1"])
  }

  function test_failed_validation_drops_the_selection() {
    subject.open("/missing", "/missing/file", false)
    fakeService.locationValidationFinished("screen-1", false, "/missing", "No such directory", "")
    compare(selections, [])
    compare(subject.pendingPath, "")
  }

  function test_refuses_before_state_is_ready() {
    fakeService.stateReady = false
    var response = JSON.parse(subject.open("/known/dir", "", false))
    fakeService.stateReady = true
    compare(response.ok, false)
    compare(navigations, [])
  }
}
