import QtQuick

QtObject {
  id: root
  required property var owner
  required property var session
  readonly property var upstream: owner.backendClient
  readonly property bool ready: upstream.ready
  readonly property bool stalled: upstream.stalled
  readonly property bool versionSkew: upstream.versionSkew
  readonly property string backendVersion: upstream.backendVersion
  readonly property var backendLog: upstream.backendLog
  readonly property var limits: upstream.limits
  readonly property string lastError: upstream.lastError
  property int serial: 0
  property var pending: ({})

  function retry() { upstream.retry() }

  function request(command, argv, generation, callback, progress, deadlineMs, options) {
    var id = "chooser-" + session.handle + "-" + (++serial)
    var entry = { id: "", generation: generation, callback: callback }
    pending[id] = entry
    var allowed = ["children-batch", "children-window", "git-metadata-batch", "search", "stat", "stat-batch", "read-text", "preferences-read", "plugin-catalog", "hypr-option", "font-match", "save-target", "mounts", "project-root", "agents", "frecency-list", "quicknav", "keybindings-prepare"]
    if (allowed.indexOf(command) < 0) {
      Qt.callLater(function() { root.finish(id, { ok: false, error: "This action is unavailable in a file chooser" }) })
      return id
    }
    if (command === "keybindings-prepare") {
      command = "read-text"
      argv = ["--path", owner.bladeHost.configDir + "/keybindings.json", "--limit", "262144"]
    }
    entry.id = upstream.request(command, argv, generation, function(response) {
      if (!root.pending[id]) return
      var rows = []
      root.collectRows(response, rows)
      if (!root.session.filter || !rows.length) { root.finish(id, response); return }
      entry.id = root.upstream.request("chooser", ["filter", "--document", JSON.stringify({ filter: root.session.filter, entries: rows })], generation, function(filtered) {
        if (!root.pending[id]) return
        if (!filtered || !filtered.ok) { root.finish(id, filtered); return }
        var allowed = Object.assign({}, root.session.allowedPaths)
        for (var path of filtered.paths) allowed[path] = true
        root.session.allowedPaths = allowed
        root.finish(id, response)
      })
    }, progress, deadlineMs, options)
    return id
  }

  function collectRows(value, rows) {
    if (!value || typeof value !== "object") return
    if (typeof value.path === "string" && typeof value.name === "string")
      rows.push({ path: value.path, name: value.name, mime: String(value.mime || "application/octet-stream") })
    for (var key of Object.keys(value)) if (typeof value[key] === "object") collectRows(value[key], rows)
  }

  function finish(id, response) {
    var entry = pending[id]
    if (!entry) return
    delete pending[id]
    if (typeof entry.callback === "function") entry.callback(response)
  }

  function cancel(id, generation, discard) {
    var entry = pending[id]
    if (entry) {
      delete pending[id]
      return upstream.cancel(entry.id, entry.generation, true)
    }
    return false
  }

  function subscribe(paths, generation, eventCallback, readyCallback, closedCallback) {
    return subscribeTopic("filesystem", paths, generation, eventCallback, readyCallback, closedCallback)
  }

  function subscribeTopic(topic, paths, generation, eventCallback, readyCallback, closedCallback) {
    var id = "chooser-watch-" + session.handle + "-" + (++serial)
    pending[id] = { generation: generation, callback: closedCallback, id: upstream.subscribeTopic(topic, paths, generation, eventCallback, readyCallback, function(response) { root.finish(id, response) }) }
    return id
  }

  Component.onDestruction: {
    for (var id of Object.keys(pending)) cancel(id, pending[id].generation, true)
  }
}
