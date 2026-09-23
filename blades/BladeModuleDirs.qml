import QtQuick
import Quickshell
import "../lib/Definitions.js" as Definitions

QtObject {
  id: dirs

  property var host: null
  property var service: null
  property var known: Object.create(null)
  property int revision: 0
  readonly property int maximumWaiters: 64
  readonly property int maximumModules: 128
  readonly property int maximumErrorLength: 256
  readonly property string stateHome: {
    var value = String(Quickshell.env("XDG_STATE_HOME") || "")
    return value.charAt(0) === "/" ? value : ((host ? host.home : "") + "/.local/state")
  }
  readonly property string stateRoot: stateHome + "/omarchy/filetree/modules/"
  readonly property string configRoot: (host ? host.configDir : "") + "/config/"

  function settled(name) {
    var entry = revision >= 0 && name ? known[name] : null
    return entry && entry.ready ? entry : null
  }

  function stateDir(id) {
    var name = Definitions.dirName(id)
    if (!name) return ""
    var entry = settled(name)
    return entry ? entry.result.stateDir : stateRoot + name
  }

  function configDir(id) {
    var name = Definitions.dirName(id)
    if (!name) return ""
    var entry = settled(name)
    return entry ? entry.result.configDir : configRoot + name
  }

  function ready(id) {
    return !!settled(Definitions.dirName(id))
  }

  function deliver(callback, result) {
    if (typeof callback === "function") callback(result)
  }

  function failure(message) {
    return { ok: false, error: Definitions.boundedText(message, "module directories were not created", maximumErrorLength) }
  }

  function ensure(id, callback) {
    var name = Definitions.dirName(id)
    if (!name) {
      deliver(callback, failure("invalid module id"))
      return false
    }
    var entry = known[name]
    if (entry && entry.ready) {
      deliver(callback, entry.result)
      return true
    }
    if (entry) {
      if (typeof callback !== "function") return true
      if (entry.pending.length >= maximumWaiters) {
        deliver(callback, failure("too many callers wait for " + name))
        return false
      }
      entry.pending.push(callback)
      return true
    }
    if (Object.keys(known).length >= maximumModules) {
      deliver(callback, failure("too many module directories"))
      return false
    }
    if (!service || typeof service.backendRequest !== "function") {
      deliver(callback, failure("backend is not available"))
      return false
    }
    known[name] = { ready: false, result: null, pending: typeof callback === "function" ? [callback] : [] }
    service.backendRequest("module-dirs", ["--module", String(id)], "module-dirs", function(response) {
      settle(name, response)
    })
    return true
  }

  function settle(name, response) {
    var entry = known[name]
    if (!entry) return
    var ok = !!(response && response.ok)
    var result = ok
      ? { ok: true, module: String(response.module || ""), name: name, stateDir: String(response.state_dir || ""), configDir: String(response.config_dir || "") }
      : failure(response && response.error)
    if (ok) {
      entry.ready = true
      entry.result = result
    } else {
      delete known[name]
    }
    revision++
    var waiting = entry.pending
    entry.pending = []
    for (var index = 0; index < waiting.length; index++) deliver(waiting[index], result)
  }
}
