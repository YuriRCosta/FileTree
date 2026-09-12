import QtQuick

QtObject {
  id: catalog

  property var service: null
  property var watchPaths: []
  property int generation: 0
  property var providers: []
  property string activation: "unknown"
  property string error: ""
  property bool loading: false
  property double checkedAt: 0
  property bool dirty: false
  property bool complete: true
  property int watchFailures: 0
  property string watchRequestId: ""
  readonly property int maximumProviders: 128
  readonly property int minimumIntervalMs: 2000
  readonly property int maximumAgeMs: 5000

  signal refreshed()

  function accepted(response) {
    if (!response || response.ok !== true || !Array.isArray(response.providers)) return null
    var rows = []
    for (var i = 0; i < response.providers.length && rows.length < maximumProviders; i++) {
      var row = response.providers[i]
      if (!row || typeof row !== "object") continue
      var id = String(row.id || "")
      var dir = String(row.dir || "")
      if (!id || !dir || !row.manifest || typeof row.manifest !== "object") continue
      rows.push({ id: id, dir: dir, manifest: row.manifest, enabled: row.enabled === true })
    }
    return rows
  }

  function withoutAuthority(rows) {
    var invalidated = []
    for (var i = 0; i < rows.length; i++)
      invalidated.push({ id: rows[i].id, dir: rows[i].dir, manifest: rows[i].manifest, enabled: false })
    return invalidated
  }

  function invalidate(message) {
    catalog.error = String(message || "The installed extensions could not be read")
    catalog.activation = "unknown"
    var invalidated = withoutAuthority(Array.isArray(providers) ? providers : [])
    catalog.providers = invalidated
    catalog.refreshed()
  }

  function stale() {
    return checkedAt <= 0 || Date.now() - checkedAt >= maximumAgeMs
  }

  function refreshIfStale() {
    return stale() ? refresh() : false
  }

  function refresh() {
    if (!service || loading) return false
    if (checkedAt > 0 && Date.now() - checkedAt < minimumIntervalMs) return false
    return read()
  }

  function read() {
    if (!service || loading) return false
    loading = true
    service.backendRequest("plugin-catalog", [], catalog.generation, function(response) {
      catalog.loading = false
      catalog.checkedAt = Date.now()
      var rows = catalog.accepted(response)
      if (rows === null) {
        catalog.invalidate(response && (response.message || response.error))
        catalog.pump()
        return
      }
      catalog.error = ""
      catalog.complete = response.complete !== false
      catalog.activation = String(response.activation || "unknown")
      var trusted = catalog.activation === "known" && catalog.complete
      catalog.providers = trusted ? rows : catalog.withoutAuthority(rows)
      catalog.refreshed()
      catalog.pump()
    })
    return true
  }

  function relevant(event) {
    if (!event) return false
    if (event.overflow) return true
    var path = String(event.path || event.root || "")
    if (!path) return false
    if (watchPaths.length === 4) {
      if (watchPaths.indexOf(path) >= 0) {
        unwatch()
        watch()
        return true
      }
      return path.indexOf(watchPaths[2] + "/") === 0 || path === watchPaths[3] + "/settings.json"
    }
    return path.indexOf("/plugins") >= 0 || path.indexOf("shell.json") >= 0
  }

  function requestRefresh() {
    dirty = true
    return pump()
  }

  function pump() {
    if (!dirty || loading || !service) return false
    var since = checkedAt > 0 ? Date.now() - checkedAt : minimumIntervalMs
    if (since < minimumIntervalMs) {
      trailing.interval = Math.max(1, minimumIntervalMs - since)
      trailing.restart()
      return false
    }
    dirty = false
    return read()
  }

  property Timer recovery: Timer {
    interval: catalog.minimumIntervalMs
    repeat: false
    onTriggered: {
      catalog.watch()
      catalog.requestRefresh()
    }
  }

  property Timer trailing: Timer {
    interval: catalog.minimumIntervalMs
    repeat: false
    onTriggered: catalog.pump()
  }

  function watch() {
    if (!service || watchRequestId || !Array.isArray(watchPaths) || watchPaths.length === 0) return false
    generation++
    var current = generation
    watchRequestId = service.backendSubscribe(watchPaths.slice(), current, function(event) {
      if (current !== catalog.generation) return
      if (catalog.relevant(event)) catalog.requestRefresh()
    }, function() {
      if (current !== catalog.generation) return
      catalog.watchFailures = 0
      catalog.refresh()
    }, function() {
      if (current !== catalog.generation) return
      catalog.watchRequestId = ""
      catalog.invalidate("The installed extensions are no longer being watched")
      catalog.watchFailures++
      recovery.interval = Math.min(60000, minimumIntervalMs * Math.pow(2, Math.min(5, catalog.watchFailures)))
      recovery.restart()
    })
    return watchRequestId !== ""
  }

  function unwatch() {
    if (!service || !watchRequestId) return
    service.cancelBackendRequest(watchRequestId, generation, true)
    watchRequestId = ""
    generation++
  }
}
