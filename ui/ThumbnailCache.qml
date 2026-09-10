import QtQuick
import "../lib/PathText.js" as PathText

QtObject {
  id: cache

  property var files: null
  property int maximumEntries: 4096
  property int maximumPending: 1024
  property int generation: 0
  property int readyCount: 0
  property var ready: ({})
  property var inflight: ({})

  function keyFor(path, stamp, edge) {
    return JSON.stringify([String(path || ""), String(stamp || ""), Math.max(1, Math.floor(Number(edge) || 1))])
  }

  function url(path, stamp, edge) {
    return String(ready[keyFor(path, stamp, edge)] || "")
  }

  function remember(key, value) {
    if (readyCount >= maximumEntries) {
      ready = ({})
      readyCount = 0
    }
    ready[key] = value
    readyCount++
  }

  function request(path, stamp, edge, callback) {
    var key = keyFor(path, stamp, edge)
    var known = ready[key]
    if (known) {
      callback({ ok: true, url: known })
      return true
    }
    if (!files || typeof files.backendRequest !== "function" || String(path || "") === "") {
      callback({ ok: false, error: "Thumbnails are unavailable" })
      return false
    }
    var flight = inflight[key]
    if (flight) {
      if (flight.callbacks.length >= Math.min(maximumPending, 128)) {
        callback({ ok: false, error: "Thumbnail queue is full" })
        return false
      }
      flight.callbacks.push(callback)
      return true
    }
    if (Object.keys(inflight).length >= maximumPending) {
      callback({ ok: false, error: "Thumbnail queue is full" })
      return false
    }
    var size = String(Math.max(1, Math.floor(Number(edge) || 1)))
    var generationAtSend = generation
    flight = { files: files, id: "", generation: generationAtSend, callbacks: [callback] }
    inflight[key] = flight
    flight.id = files.backendRequest("thumbnail", ["--path", String(path), "--key", key, "--width", size, "--height", size], generationAtSend, function(response) {
      if (!cache || generationAtSend !== cache.generation) return
      var entry = cache.inflight[key]
      if (entry !== flight) return
      delete cache.inflight[key]
      var okay = !!(response && response.ok && String(response.path || "") !== "")
      var result = okay
        ? { ok: true, url: PathText.fileUrl(String(response.path)) }
        : { ok: false, error: String(response && response.error || "Preview unavailable") }
      if (okay) cache.remember(key, result.url)
      for (var i = 0; i < entry.callbacks.length; i++) entry.callbacks[i](result)
    })
    return true
  }

  function cancel(callback) {
    var keys = Object.keys(inflight)
    for (var i = 0; i < keys.length; i++) {
      var flight = inflight[keys[i]]
      flight.callbacks = flight.callbacks.filter(function(value) { return value !== callback })
      if (flight.callbacks.length > 0) continue
      delete inflight[keys[i]]
      if (flight.id && flight.files && typeof flight.files.cancelBackendRequest === "function")
        flight.files.cancelBackendRequest(flight.id, flight.generation, true)
    }
  }

  function reset() {
    generation++
    ready = ({})
    readyCount = 0
    var keys = Object.keys(inflight)
    var pending = inflight
    inflight = ({})
    for (var i = 0; i < keys.length; i++) {
      var flight = pending[keys[i]]
      if (flight && flight.id && flight.files && typeof flight.files.cancelBackendRequest === "function")
        flight.files.cancelBackendRequest(flight.id, flight.generation, true)
    }
    inflight = ({})
  }

  onFilesChanged: reset()
  Component.onDestruction: reset()
}
