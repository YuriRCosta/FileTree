import QtQuick
import QtTest
import "../../controllers" as Controllers

TestCase {
  id: testCase
  name: "LaunchController"

  property var subject: null
  property var requests: []
  property var navigations: []
  property var events: []
  property int focusYields: 0

  QtObject {
    id: fakeService
    property bool open: true
    property string selectedPath: ""

    function rootName(path) { return String(path).split("/").pop() }
    function setOpen(value) { open = !!value }
    function recordFrecencyVisit(arguments) {}

    function yieldFocusForExternalLaunch() {
      testCase.focusYields++
      testCase.events = testCase.events.concat(["yield-focus"])
    }

    function navigateToLocation(path, screen, mode) {
      testCase.navigations = testCase.navigations.concat([{ path: path, screen: screen, mode: mode }])
      return "checking"
    }

    function backendRequest(name, arguments, generation, callback) {
      testCase.events = testCase.events.concat(["request-" + name])
      testCase.requests = testCase.requests.concat([{
        name: name,
        arguments: arguments,
        generation: generation,
        callback: callback
      }])
      return "request-" + testCase.requests.length
    }
  }

  Component {
    id: launchComponent
    Controllers.LaunchController { service: fakeService }
  }

  function init() {
    requests = []
    navigations = []
    events = []
    focusYields = 0
    fakeService.open = true
    subject = launchComponent.createObject(testCase)
    verify(subject !== null)
  }

  function cleanup() {
    subject.destroy()
    subject = null
  }

  function test_known_directory_never_reaches_external_launcher() {
    var screen = ({ name: "plugin-screen" })
    verify(subject.enqueue("/tmp/folder", "default", "", 0, screen, true))
    compare(requests.length, 0)
    compare(navigations.length, 1)
    compare(navigations[0].path, "/tmp/folder")
    compare(navigations[0].screen, screen)
    compare(navigations[0].mode, "browse")
    compare(subject.lastPath, "/tmp/folder")
    compare(subject.status, "Opened in FileTree")
    compare(focusYields, 0)
  }

  function test_unknown_directory_is_probed_then_opened_in_filetree() {
    verify(subject.enqueue("/tmp/plugin-folder", "default", ""))
    compare(requests.length, 1)
    compare(requests[0].name, "stat-batch")

    requests[0].callback({ ok: true, entries: [{ path: "/tmp/plugin-folder", is_dir: true }] })

    compare(requests.length, 1)
    compare(navigations.length, 1)
    compare(navigations[0].path, "/tmp/plugin-folder")
    compare(focusYields, 0)
  }

  function test_known_file_keeps_external_default_application() {
    verify(subject.enqueue("/tmp/file.txt", "default", "", 0, null, false))
    compare(navigations.length, 0)
    compare(requests.length, 1)
    compare(requests[0].name, "launch")
    compare(requests[0].arguments.join(" "), "--path /tmp/file.txt --mode default")
    compare(focusYields, 1)
    compare(events.join(","), "yield-focus,request-launch")
  }

  function test_unknown_file_yields_only_after_the_probe_routes_it_externally() {
    verify(subject.enqueue("/tmp/plugin-file.txt", "default", ""))
    compare(requests.length, 1)
    compare(requests[0].name, "stat-batch")
    compare(focusYields, 0)

    requests[0].callback({ ok: true, entries: [{ path: "/tmp/plugin-file.txt", is_dir: false }] })

    compare(requests.length, 2)
    compare(requests[1].name, "launch")
    compare(focusYields, 1)
    compare(events.join(","), "request-stat-batch,yield-focus,request-launch")
  }

  function test_every_explicit_external_mode_yields_focus_data() {
    return [
      { tag: "editor", mode: "editor", desktopId: "" },
      { tag: "application", mode: "application", desktopId: "org.example.App.desktop" },
      { tag: "reveal", mode: "reveal", desktopId: "" }
    ]
  }

  function test_every_explicit_external_mode_yields_focus(data) {
    verify(subject.enqueue("/tmp/item", data.mode, data.desktopId))
    compare(requests.length, 1)
    compare(requests[0].name, "launch")
    compare(focusYields, 1)
    compare(events.join(","), "yield-focus,request-launch")
  }

  function test_multiple_external_launches_are_serial_and_each_yields_focus() {
    verify(subject.enqueue("/tmp/first.txt", "editor", ""))
    verify(subject.enqueue("/tmp/second.txt", "editor", ""))
    compare(requests.length, 1)
    compare(focusYields, 1)

    requests[0].callback({ ok: true })

    tryCompare(testCase, "focusYields", 2)
    compare(requests.length, 2)
    compare(requests[1].arguments.join(" "), "--path /tmp/second.txt --mode editor")
    compare(events.join(","), "yield-focus,request-launch,yield-focus,request-launch")
  }

  function test_directory_open_restores_closed_filetree() {
    fakeService.open = false
    verify(subject.enqueue("/tmp/folder", "default", "", 0, null, true))
    verify(fakeService.open)
    compare(navigations.length, 1)
    compare(focusYields, 0)
  }
}
