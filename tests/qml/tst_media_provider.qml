import QtQuick
import QtTest
import "../../modules/files" as Files

TestCase {
  id: test
  name: "MediaProvider"
  property var calls: []
  property var callbacks: []
  property var canceled: []

  QtObject {
    id: backend
    property string rootPath: "/fixture"
    property bool showHidden: false
    property bool gitEnabled: false
    function makeRow(entry, depth) {
      return Object.assign({}, entry, { isDir: !!entry.is_dir, isSymlink: !!entry.is_symlink })
    }
    function backendRequest(name, args, generation, callback) {
      test.calls.push({ name: name, args: args, generation: generation })
      test.callbacks.push(callback)
      return String(test.calls.length)
    }
    function cancelBackendRequest(id, generation, discard) { test.canceled.push(id) }
  }

  Component { id: factory; Files.MediaProvider { controller: backend } }

  function init() { calls = []; callbacks = []; canceled = [] }

  function test_current_folder_does_not_recurse_and_keeps_video() {
    var provider = createTemporaryObject(factory, test)
    provider.active = true
    tryCompare(provider, "requestId", "1")
    callbacks[0]({ ok: true, entries: [
      { path: "/fixture/nested", name: "nested", is_dir: true },
      { path: "/fixture/a.png", name: "a.png", mime: "image/png" },
      { path: "/fixture/b.mkv", name: "b.mkv" },
      { path: "/fixture/notes.txt", name: "notes.txt" }
    ] })
    tryCompare(provider, "busy", false)
    compare(calls.length, 1)
    compare(provider.rows.length, 2)
  }

  function test_paging_and_explicit_recursion_skip_symlink_cycles() {
    var provider = createTemporaryObject(factory, test, { recursive: true })
    provider.active = true
    tryCompare(provider, "requestId", "1")
    callbacks[0]({ ok: true, start: 0, truncated: true, entries: [
      { path: "/fixture/nested", name: "nested", is_dir: true },
      { path: "/fixture/cycle", name: "cycle", is_dir: true, is_symlink: true }
    ] })
    tryCompare(provider, "requestId", "2")
    compare(calls[1].args[3], "400")
    callbacks[1]({ ok: true, entries: [] })
    tryCompare(provider, "requestId", "3")
    compare(calls[2].args[1], "/fixture/nested")
    callbacks[2]({ ok: true, entries: [] })
    tryCompare(provider, "busy", false)
    compare(calls.length, 3)
  }

  function test_close_cancels_and_late_results_cannot_repopulate() {
    var provider = createTemporaryObject(factory, test)
    provider.active = true
    tryCompare(provider, "requestId", "1")
    provider.active = false
    compare(canceled, ["1"])
    callbacks[0]({ ok: true, entries: [{ path: "/late.png", name: "late.png" }] })
    compare(provider.rows.length, 0)
    compare(provider.busy, false)
  }

  function test_failed_or_denied_location_is_distinct_from_empty() {
    var provider = createTemporaryObject(factory, test, { descriptor: { capabilities: [] } })
    provider.active = true
    verify(provider.error !== "")
    compare(calls.length, 0)
    provider.descriptor = null
    tryCompare(provider, "requestId", "1")
    callbacks[0]({ ok: false, error: "Permission denied" })
    tryCompare(provider, "busy", false)
    compare(provider.error, "Permission denied")
  }
}
