import QtQuick
import qs.Commons

QtObject {
  id: cache

  property var files: null
  property int maximumEntries: 4096
  property int generation: 0
  property int readyCount: 0
  property var ready: ({})
  property var inflight: ({})

  function keyFor(path, stamp, edge) {
    return String(path || "") + "\n" + String(stamp || "") + "\n" + String(Math.max(1, Math.floor(Number(edge) || 1)))
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
      flight.callbacks.push(callback)
      return true
    }
    var size = String(Math.max(1, Math.floor(Number(edge) || 1)))
    var generationAtSend = generation
    flight = { id: "", generation: generationAtSend, callbacks: [callback] }
    inflight[key] = flight
    flight.id = files.backendRequest("thumbnail", ["--path", String(path), "--key", key, "--width", size, "--height", size], generationAtSend, function(response) {
      if (!cache || generationAtSend !== cache.generation) return
      var entry = cache.inflight[key]
      if (!entry) return
      delete cache.inflight[key]
      var okay = !!(response && response.ok && String(response.path || "") !== "")
      var result = okay
        ? { ok: true, url: Util.fileUrl(String(response.path)) }
        : { ok: false, error: String(response && response.error || "Preview unavailable") }
      if (okay) cache.remember(key, result.url)
      for (var i = 0; i < entry.callbacks.length; i++) entry.callbacks[i](result)
    })
    return true
  }

  function reset() {
    generation++
    var keys = Object.keys(inflight)
    for (var i = 0; i < keys.length; i++) {
      var flight = inflight[keys[i]]
      if (flight && flight.id && files && typeof files.cancelBackendRequest === "function")
        files.cancelBackendRequest(flight.id, flight.generation, true)
    }
    inflight = ({})
  }

  onFilesChanged: reset()
  Component.onDestruction: reset()
}
