import QtQuick
import Quickshell.Io

Item {
  id: probe
  property var subject: null
  property var response: null
  property int completions: 0
  readonly property var inventory: subject ? subject.inventory : null

  function binOf(item) {
    if (!item) return null
    if (item.removalArguments !== undefined && item.choose !== undefined) return item
    var children = item.children || []
    for (var i = 0; i < children.length; i++) {
      var found = binOf(children[i])
      if (found) return found
    }
    return null
  }

  function rows() {
    if (!subject) return []
    return subject.items !== undefined ? subject.items : subject.definitions
  }

  function snapshot() {
    var bin = binOf(subject)
    return {
      module: subject && subject.context ? subject.context.moduleId : "",
      provider: inventory ? inventory.providerId : "",
      providerRoot: inventory ? inventory.providerRoot : "",
      observers: subject && subject.provider ? subject.provider.viewCount : -1,
      ready: !!inventory && inventory.ready,
      busy: !!inventory && inventory.busy,
      applying: !!inventory && inventory.applying,
      loadError: inventory ? inventory.loadError : "no inventory",
      applyError: inventory ? inventory.applyError : "",
      watchError: inventory ? inventory.watchError : "",
      lanes: inventory ? inventory.lanes.map(function(lane) {
        return { scope: lane.scope, scan: !!lane.scan, watch: !!lane.watch, stopping: lane.stopping }
      }) : [],
      rows: rows().filter(function(row) { return row.scope === "project" }).slice(0, 30),
      bin: bin ? { rows: bin.rows, busy: bin.busy, error: bin.error, route: bin.helperRoute } : null,
      consent: !!subject && subject.files.agentManagementEnabled,
      completions: completions,
      response: response
    }
  }

  Connections {
    target: probe.inventory
    function onMutationFinished(method, result, project) {
      probe.response = result
      probe.completions++
    }
  }

  Connections {
    target: probe.subject ? probe.subject.files.artifactActions : null
    function onFinished(module, requestId, response) {
      if (module !== probe.subject.context.moduleId) return
      probe.response = response
      probe.completions++
    }
  }

  IpcHandler {
    target: probe.subject && probe.subject.context ? "fileblade.core-live." + probe.subject.context.moduleId : ""
    function status(): string { return JSON.stringify(probe.snapshot()) }
    function consent(enabled: bool): string {
      return String(probe.subject.files.preferences.setAgentManagement(enabled))
    }
    function projectContext(): string { return String(probe.subject.files.setProjectContext(true)) }
    function apply(id: string, agent: string, enabled: bool): string {
      var row = probe.rows().find(function(value) { return String(value.id) === id && value.scope === "project" })
      if (!row) return "missing fixture row"
      probe.response = null
      if (probe.subject.applyAgents !== undefined) probe.subject.applyAgents(row, [agent], enabled)
      else probe.subject.runApply(row, [agent], enabled ? "on" : "off")
      return "queued"
    }
    function bin(id: string, action: string): string {
      var bin = probe.binOf(probe.subject)
      if (!bin || ["bin", "restore", "purge", "ask"].indexOf(action) < 0) return "unavailable"
      var row = (action === "bin" ? probe.rows() : bin.rows).find(function(value) { return String(value.id) === id })
      if (!row) return "missing fixture row"
      if (action === "ask") { bin.ask(row); return "opened" }
      bin.pending = row
      bin.choose(action)
      return "queued"
    }
    function refresh(): string {
      probe.subject.refresh()
      var bin = probe.binOf(probe.subject)
      if (bin) bin.refresh()
      return "queued"
    }
  }
}
