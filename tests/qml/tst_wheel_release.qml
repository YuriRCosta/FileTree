import QtQuick
import QtTest

TestCase {
  id: suite
  name: "WheelRelease"
  property var wheel: null
  property var callbacks: []
  property var launches: []

  QtObject {
    id: service
    property var bladeHost: QtObject {
      property bool pressActive: false
      function windowTitle(edge) { return edge }
    }
    function backendRequest(command, args, generation, callback) {
      if (command === "drop-context") suite.callbacks.push(callback)
      else suite.launches.push(args)
      return String(suite.callbacks.length + suite.launches.length)
    }
    function cancelBackendRequest(id, generation) {}
    function yieldFocusForExternalLaunch() {}
    function referenceScreen(value) { return null }
  }

  function initTestCase() {
    var url = Qt.resolvedUrl("../../controllers/DropWheelController.qml")
    var request = new XMLHttpRequest()
    request.open("GET", url, false)
    request.send()
    var source = request.responseText.replace("import Quickshell\n", "")
    source = source.slice(0, source.lastIndexOf("  Variants {")) + "}\n"
    wheel = Qt.createQmlObject(source.replace("required property var service", "property var service"), suite, url)
    wheel.service = service
  }

  function init() {
    wheel.close()
    callbacks = []
    launches = []
    wheel.dragActive = false
  }

  function cleanupTestCase() { wheel.destroy() }

  function begin() {
    verify(wheel.beginDrag(["/tmp/a.txt"], [], null, true, 500, 500, null))
    wheel.openWheel(null, 500, 500, true)
  }

  function result() {
    return { ok: true, target: { kind: "desktop" }, files: { paths: ["/tmp/a.txt"], files: ["/tmp/a.txt"], count: 1 }, actions: [
      { id: "terminal", label: "Terminal", key: "t", placements: [] },
      { id: "open", label: "Open", key: "o", placements: [] }
    ] }
  }

  function test_early_release_runs_once_when_rows_arrive() {
    begin()
    verify(wheel.endDrag(500, 440, true))
    compare(launches.length, 0)
    verify(wheel.wheelOpen)
    callbacks[0](result())
    compare(launches.length, 1)
    compare(launches[0][1], "terminal")
    compare(wheel.pendingRelease, null)
    wait(850)
    compare(launches.length, 1)
  }

  function test_timeout_keeps_clickable_wheel() {
    begin()
    wheel.endDrag(500, 440, true)
    tryCompare(wheel, "pendingRelease", null, 1200)
    callbacks[0](result())
    verify(wheel.wheelOpen)
    compare(launches.length, 0)
    compare(wheel.status, "Choose an action to continue")
    verify(wheel.activateAt(500, 440))
    compare(launches.length, 1)
  }

  function test_expired_deadline_before_timer_delivery_does_not_launch() {
    begin()
    wheel.endDrag(500, 440, true)
    wheel.pendingRelease = { x: 500, y: 440, generation: wheel.contextGeneration, deadline: Date.now() - 1 }
    callbacks[0](result())
    verify(wheel.wheelOpen)
    compare(launches.length, 0)
  }

  function test_hub_release_waits_for_a_choice() {
    begin()
    wheel.endDrag(500, 500, true)
    callbacks[0](result())
    verify(wheel.wheelOpen)
    compare(launches.length, 0)
    compare(wheel.highlighted, -1)
  }

  function test_closed_wheel_discards_late_response() {
    begin()
    wheel.endDrag(500, 440, true)
    wheel.close()
    callbacks[0](result())
    verify(!wheel.wheelOpen)
    compare(launches.length, 0)
    compare(wheel.pendingRelease, null)
  }

  function test_reopened_wheel_ignores_old_context() {
    begin()
    wheel.endDrag(500, 440, true)
    wheel.openWheel(null, 700, 700, false)
    callbacks[0](result())
    verify(wheel.loading)
    compare(wheel.ringItems.length, 0)
    callbacks[1](result())
    verify(wheel.wheelOpen)
    compare(launches.length, 0)
  }

  function test_failed_context_keeps_error_without_launch() {
    begin()
    wheel.endDrag(500, 440, true)
    callbacks[0]({ ok: false, error: "Unavailable" })
    compare(wheel.pendingRelease, null)
    compare(wheel.error, "Unavailable")
    verify(wheel.wheelOpen)
    compare(launches.length, 0)
  }
  function nestedResult() {
    var value = result()
    value.actions = [{ id: "custom:inspect", label: "Inspect", key: "i", placements: [
      { id: "format", label: "Format", key: "f", placements: [
        { id: "one", label: "One", key: "a", command_route: ["custom:inspect", "format", "one"] },
        { id: "two", label: "Two", key: "b", command_route: ["custom:inspect", "format", "two"] }
      ] }
    ] }]
    return value
  }

  function test_third_ring_keyboard_dispatches_exact_route() {
    begin()
    wheel.endDrag(500, 500, true)
    callbacks[0](nestedResult())
    verify(wheel.activateKey("i", false))
    verify(wheel.outerFocus)
    verify(wheel.activateKey("f", false))
    verify(wheel.subFocus)
    wheel.moveHighlight(1)
    compare(wheel.subHighlighted, 1)
    verify(wheel.accept())
    compare(launches.length, 1)
    compare(launches[0][1], "configured")
    compare(JSON.parse(launches[0][3]), ["custom:inspect", "format", "two"])
  }

  function test_third_ring_back_returns_one_level_at_a_time() {
    begin()
    wheel.endDrag(500, 500, true)
    callbacks[0](nestedResult())
    wheel.activate(0)
    wheel.activateChild(0)
    wheel.back()
    verify(wheel.wheelOpen)
    verify(!wheel.subFocus)
    verify(wheel.outerFocus)
    compare(wheel.subItems.length, 0)
    wheel.back()
    verify(wheel.wheelOpen)
    verify(!wheel.outerFocus)
    wheel.back()
    verify(!wheel.wheelOpen)
    compare(launches.length, 0)
  }

  function test_third_ring_pointer_and_parent_change() {
    begin()
    wheel.endDrag(500, 500, true)
    var value = nestedResult()
    value.actions.push({ id: "terminal", label: "Terminal", key: "t", placements: [] })
    callbacks[0](value)
    wheel.hover(500, 440)
    wheel.hover(500, 380)
    compare(wheel.subItems.length, 2)
    var angle = wheel.subAngle(1)
    var x = 500 + Math.cos(angle) * 170
    var y = 500 + Math.sin(angle) * 170
    wheel.hover(x, y)
    compare(wheel.subHighlighted, 1)
    wheel.hover(500, 440)
    compare(wheel.subItems.length, 0)
    wheel.hover(500, 380)
    compare(wheel.subItems.length, 2)
    wheel.hover(500, 560)
    compare(wheel.subItems.length, 0)
    compare(wheel.subHighlighted, -1)
  }
  function test_explicit_hub_choice_discards_queued_release() {
    begin()
    wheel.endDrag(500, 440, true)
    wheel.activateAt(500, 500)
    callbacks[0](result())
    verify(wheel.wheelOpen)
    compare(launches.length, 0)
    compare(wheel.pendingRelease, null)
  }
}
