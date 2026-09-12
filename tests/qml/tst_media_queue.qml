import QtQuick
import QtTest
import "../../ui" as PluginUi

TestCase {
  name: "MediaQueue"

  QtObject {
    id: backend
    property var sent: []
    property var canceled: []
    function backendRequest(method, args, generation, callback) {
      var id = String(sent.length + 1)
      sent.push({ id: id, method: method, args: args, generation: generation, callback: callback })
      return id
    }
    function cancelBackendRequest(id, generation, quiet) { canceled.push(id) }
  }

  PluginUi.ThumbnailCache { id: cache; files: backend; maximumPending: 6 }

  function init() {
    cache.reset()
    backend.sent = []
    backend.canceled = []
  }

  function test_four_active_and_bounded_waiting_preserve_image_wire_bytes() {
    var received = []
    for (var i = 0; i < 6; i++) verify(cache.request("/" + i + ".png", "stamp", 256, function(r) { received.push(r) }))
    compare(backend.sent.length, 4)
    compare(Object.keys(cache.inflight).length, 6)
    compare(cache.request("/overflow.png", "stamp", 256, function(r) { compare(r.error, "Thumbnail queue is full") }), false)
    compare(backend.sent[0].method, "thumbnail")
    compare(backend.sent[0].args, ["--path", "/0.png", "--key", JSON.stringify(["/0.png", "stamp", 256]), "--width", "256", "--height", "256"])
    for (var n = 0; n < 6; n++) {
      backend.sent[n].callback({ ok: true, path: "/cache/" + n + " #%.png" })
      compare(backend.sent.length, Math.min(6, n + 5))
    }
    compare(Object.keys(cache.inflight).length, 0)
    compare(received[0], { ok: true, url: "file:///cache/0%20%23%25.png" })
    verify(cache.request("/0.png", "stamp", 256, function(r) { compare(r, received[0]) }))
    compare(backend.sent.length, 6)
  }

  function test_cancel_queued_and_last_active_subscriber_drains_and_ignores_late_results() {
    var callbacks = [], received = []
    for (var i = 0; i < 6; i++) {
      callbacks.push(function(r) { received.push(r) })
      cache.request("/" + i + ".webp", "stamp", 256, callbacks[i])
    }
    cache.cancel(callbacks[4])
    compare(backend.sent.length, 4)
    compare(backend.canceled, [])
    var shared = function(r) { received.push(r) }
    cache.request("/0.webp", "stamp", 256, shared)
    cache.cancel(callbacks[0])
    compare(backend.canceled, [])
    cache.cancel(shared)
    compare(backend.canceled, ["1"])
    compare(backend.sent.length, 5)
    compare(backend.sent[4].args[1], "/5.webp")
    backend.sent[0].callback({ ok: true, path: "/late.png" })
    compare(received, [])
    compare(Object.keys(cache.inflight).length, 4)
    cache.request("/replacement.jpg", "stamp", 256, shared)
    cache.cancel(callbacks[1])
    compare(backend.sent[5].args[1], "/replacement.jpg")
    cache.files = null
    compare(Object.keys(cache.inflight).length, 0)
    backend.sent[5].callback({ ok: true, path: "/stale.jpg" })
    compare(received, [])
    cache.files = backend
    cache.request("/new.jpg", "stamp", 256, shared)
    compare(backend.sent.length, 7)
  }
}
