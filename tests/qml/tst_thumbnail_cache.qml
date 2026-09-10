import QtQuick
import QtTest
import "../../ui" as PluginUi

TestCase {
  name: "ThumbnailCache"

  QtObject {
    id: backend
    property var sent: []
    property var canceled: []
    function backendRequest(method, args, generation, callback) {
      var id = String(sent.length + 1)
      sent.push({ id: id, callback: callback })
      return id
    }
    function cancelBackendRequest(id, generation, quiet) { canceled.push(id) }
  }

  PluginUi.ThumbnailCache { id: cache; files: backend; maximumPending: 2 }

  function init() {
    cache.reset()
    backend.sent = []
    backend.canceled = []
  }

  function test_recycling_unsubscribes_and_cancels_last_request() {
    var received = []
    var first = function(result) { received.push("first") }
    var second = function(result) { received.push(result.url) }
    cache.request("/a #%.png", "1", 256, first)
    cache.request("/a #%.png", "1", 256, second)
    compare(backend.sent.length, 1)
    cache.cancel(first)
    compare(backend.canceled.length, 0)
    backend.sent[0].callback({ ok: true, path: "/cache/a #%.png" })
    compare(received, ["file:///cache/a%20%23%25.png"])
    cache.request("/b.png", "1", 256, first)
    cache.cancel(first)
    compare(backend.canceled, ["2"])
    compare(Object.keys(cache.inflight).length, 0)
  }

  function test_old_completion_cannot_satisfy_replacement() {
    var received = []
    var first = function(result) { received.push("old") }
    var second = function(result) { received.push("new") }
    cache.request("/a.png", "1", 256, first)
    cache.cancel(first)
    cache.request("/a.png", "1", 256, second)
    backend.sent[0].callback({ ok: true, path: "/cache/old.png" })
    compare(received.length, 0)
    compare(Object.keys(cache.inflight).length, 1)
    backend.sent[1].callback({ ok: true, path: "/cache/new.png" })
    compare(received, ["new"])
  }

  function test_requests_and_subscribers_are_bounded() {
    var callback = function(result) {}
    cache.request("/a.png", "1", 256, callback)
    cache.request("/a.png", "1", 256, callback)
    compare(cache.request("/a.png", "1", 256, callback), false)
    cache.request("/b.png", "1", 256, callback)
    compare(cache.request("/c.png", "1", 256, callback), false)
    compare(backend.sent.length, 2)
    cache.reset()
    compare(backend.canceled, ["1", "2"])
    compare(Object.keys(cache.inflight).length, 0)
  }

  function test_key_preserves_newline_boundaries() {
    verify(cache.keyFor("/a\nb", "c", 256) !== cache.keyFor("/a", "b\nc", 256))
  }
}
