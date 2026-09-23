import QtQuick
import "../lib/ActionRows.js" as ActionRows

Item {
  id: controller
  required property var service
  property var actions: []
  property var listErrors: []
  property var running: Object.create(null)
  property var results: Object.create(null)
  property var order: []
  property var lastResult: null
  property bool truncated: false
  property bool refreshQueued: false
  property string listError: ""
  property int generation: 0
  property int serial: 0
  readonly property string socketKey: "yuricosta.filetree/action"
  readonly property int maximumProviders: 128
  readonly property int maximumResults: 32
  readonly property int maximumLiveRuns: 4
  readonly property int deadlineGraceMs: 5000
  readonly property int maximumDeadlineMs: 900000
  readonly property int noticeLength: 200
  readonly property var registry: service && service.pluginRegistry ? service.pluginRegistry : null
  signal finished(string key, var result)
  function contributes(manifest) {
    if (!manifest || typeof manifest !== "object") return false
    var extensions = manifest.extensions
    if (!extensions || typeof extensions !== "object" || Array.isArray(extensions)) return false
    var entries = extensions[socketKey]
    return Array.isArray(entries) && entries.length > 0
  }
  property var catalogProviders: []

  function catalogRow(id) {
    var rows = Array.isArray(catalogProviders) ? catalogProviders : []
    for (var i = 0; i < rows.length; i++)
      if (rows[i] && String(rows[i].id) === String(id)) return rows[i]
    return null
  }
  function providerEnabled(id) {
    var key = String(id || "")
    var installed = registry && registry.installedPlugins ? registry.installedPlugins : ({})
    if (!installed[key]) {
      var row = catalogRow(key)
      if (row) return row.enabled === true
    }
    if (!registry || typeof registry.isEnabled !== "function") return true
    return !!registry.isEnabled(key)
  }
  function userDirectory() {
    return service && service.bladeHost ? service.bladeHost.configDir + "/actions" : ""
  }
  function providerArguments() {
    var installed = registry && registry.installedPlugins ? registry.installedPlugins : ({})
    var ids = Object.keys(installed)
    var parameters = []
    var providers = 0
    for (var i = 0; i < ids.length && providers < maximumProviders; i++) {
      var manifest = installed[ids[i]]
      if (!contributes(manifest) || !manifest.__sourceDir) continue
      if (!providerEnabled(ids[i])) continue
      parameters.push("--provider", ids[i] + "=" + String(manifest.__sourceDir))
      providers++
    }
    var catalog = Array.isArray(catalogProviders) ? catalogProviders : []
    for (var j = 0; j < catalog.length && providers < maximumProviders; j++) {
      var row = catalog[j]
      if (!row || row.enabled !== true || !row.id || !row.dir) continue
      if (installed[row.id] || !contributes(row.manifest)) continue
      parameters.push("--provider", String(row.id) + "=" + String(row.dir))
      providers++
    }
    parameters.push("--user", userDirectory())
    return parameters
  }
  function refresh() {
    if (refreshQueued) return
    refreshQueued = true
    Qt.callLater(function() {
      controller.refreshQueued = false
      controller.requestList()
    })
  }
  function requestList() {
    if (!service) return
    generation++
    var requestGeneration = generation
    service.backendRequest("action-list", providerArguments(), requestGeneration, function(response) {
      if (requestGeneration !== controller.generation) return
      controller.applyList(response)
    })
  }
  function applyList(response) {
    var payload = response && typeof response === "object" ? response : ({})
    actions = ActionRows.normalize(payload.actions)
    listErrors = ActionRows.errors(payload.errors)
    truncated = payload.truncated === true
    listError = payload.ok === false ? ActionRows.firstLine(payload.error, noticeLength) : ""
  }
  function find(key) {
    var wanted = String(key === undefined || key === null ? "" : key)
    for (var i = 0; i < actions.length; i++)
      if (actions[i].key === wanted) return actions[i]
    return null
  }
  function rowsFor(entries, rootPath) {
    return ActionRows.rowsFor(actions, entries, rootPath)
  }
  function isRunning(key) {
    return !!running[String(key === undefined || key === null ? "" : key)]
  }
  function refusal(error) {
    return { ok: false, error: String(error) }
  }
  function gate(action, context, yes, count) {
    if (!action) return "unknown action"
    if (action.source === "plugin" && !providerEnabled(action.plugin))
      return "plugin " + action.plugin + " is disabled"
    if (action.confirm && !yes) return "needs-confirm"
    if (isRunning(action.key)) return "already running"
    if (Object.keys(running).length >= maximumLiveRuns)
      return "action limit reached (" + maximumLiveRuns + ")"
    if (context === "selection" && count > ActionRows.MAXIMUM_TARGETS)
      return "selection too large (" + ActionRows.MAXIMUM_TARGETS + ")"
    if (context === "") return action.title + " does not accept this selection"
    return ""
  }
  function plan(action, entries, rootPath, screen, yes, ticket) {
    var context = ActionRows.contextOf(action, entries, rootPath)
    return {
      context: context,
      targets: ActionRows.targetsFor(context, entries, rootPath),
      root: String(rootPath === undefined || rootPath === null ? "" : rootPath),
      screen: screen && screen.name ? String(screen.name) : "",
      yes: !!yes,
      ticket: ticket ? ticket : nextTicket()
    }
  }

  function nextTicket() {
    serial++
    return "act-" + Date.now() + "-" + serial
  }

  function run(key, entries, rootPath, screen, yes) {
    var action = find(key)
    var target = plan(action, entries, rootPath, screen, yes, "")
    var count = Array.isArray(entries) ? entries.length : 0
    var error = gate(action, target.context, yes, count)
    if (error !== "") {
      if (error !== "needs-confirm") service.operationError = error
      return ""
    }
    dispatch(action, target)
    return target.ticket
  }

  function dispatch(action, target) {
    var parameters = [
      "--source", action.source,
      "--plugin", action.plugin,
      "--plugin-dir", action.pluginRoot,
      "--action", action.id,
      "--context", target.context
    ]
    if (target.root !== "") parameters.push("--root", target.root)
    for (var i = 0; i < target.targets.length; i++) parameters.push("--path", target.targets[i])
    if (target.screen !== "") parameters.push("--screen", target.screen)
    if (target.yes) parameters.push("--yes")
    var deadline = Math.min(maximumDeadlineMs, action.timeout * 1000 + deadlineGraceMs)
    claim(action.key, target.ticket)
    service.backendRequest("action-run", parameters, generation, function(response) {
      controller.complete(action, target.ticket, response)
    }, undefined, deadline)
  }

  function claim(key, ticket) {
    var next = Object.create(null)
    var keys = Object.keys(running)
    for (var i = 0; i < keys.length; i++)
      if (keys[i] !== key) next[keys[i]] = running[keys[i]]
    if (ticket !== "") next[key] = ticket
    running = next
  }

  function store(ticket, result) {
    var next = Object.create(null)
    var kept = order.slice(Math.max(0, order.length + 1 - maximumResults))
    for (var i = 0; i < kept.length; i++) next[kept[i]] = results[kept[i]]
    next[ticket] = result
    kept.push(ticket)
    results = next
    order = kept
    lastResult = result
  }

  function complete(action, ticket, response) {
    var result = response && typeof response === "object"
      ? response : refusal("the action backend did not answer")
    if (running[action.key] === ticket) claim(action.key, "")
    store(ticket, result)
    report(action, result)
    finished(action.key, result)
  }

  function outcome(result) {
    if (result.timed_out === true) return "timed out"
    if (result.detached === true) return "detached"
    if (result.exit_code === null || result.exit_code === undefined) return "killed"
    return "exit " + Math.floor(Number(result.exit_code) || 0)
  }

  function report(action, result) {
    if (result.ok !== true) {
      var detail = ActionRows.firstLine(result.stderr_tail || result.error, noticeLength)
      service.operationError = (action.title + " failed (" + outcome(result) + ")"
        + (detail === "" ? "" : ": " + detail)).slice(0, noticeLength)
      return
    }
    if (action.output === "silent") return
    var line = ActionRows.firstLine(result.stdout_tail, noticeLength)
    service.operationNotice = (action.title + ": " + outcome(result)
      + " in " + Math.max(0, Math.floor(Number(result.elapsed_ms) || 0)) + " ms"
      + (line === "" ? "" : ", " + line)).slice(0, noticeLength)
  }

  function result(ticket) {
    var id = ActionRows.firstLine(ticket, 128)
    if (id === "") return { status: "unknown", id: id }
    if (results[id]) return { status: "done", id: id, result: results[id] }
    var keys = Object.keys(running)
    for (var i = 0; i < keys.length; i++)
      if (running[keys[i]] === id) return { status: "running", id: id, key: keys[i] }
    return { status: "unknown", id: id }
  }

  function document() {
    return {
      ok: listError === "",
      error: listError,
      truncated: truncated,
      errors: listErrors,
      count: actions.length,
      running: Object.keys(running),
      actions: actions
    }
  }

  function runFromIpc(key, pathsJson, yes) {
    var action = find(key)
    if (!action) return refusal("unknown action " + ActionRows.firstLine(key, 128))
    var paths = []
    var encoded = String(pathsJson === undefined || pathsJson === null ? "[]" : pathsJson)
    var encodedLimit = encoded.indexOf("base64:") === 0
      ? ActionRows.MAXIMUM_ENCODED_PATH_DOCUMENT_LENGTH : ActionRows.MAXIMUM_PATH_DOCUMENT_LENGTH
    if (encoded.length > encodedLimit)
      return refusal("path list is too large")
    var decoded = service && typeof service.decodedJsonDocument === "function"
      ? service.decodedJsonDocument(encoded) : ({ ok: false, value: null })
    if (decoded.ok !== true || !Array.isArray(decoded.value))
      return refusal("paths must be a JSON array")
    paths = decoded.value
    if (paths.length > ActionRows.MAXIMUM_TARGETS)
      return refusal("selection too large (" + ActionRows.MAXIMUM_TARGETS + ")")
    for (var i = 0; i < paths.length; i++)
      if (typeof paths[i] !== "string" || paths[i] === "" || paths[i].length > ActionRows.MAXIMUM_PATH_LENGTH
          || paths[i].indexOf("\u0000") >= 0) return refusal("every path must be a usable string")
    var available = gate(action, "pending", yes, paths.length)
    if (available !== "") return refusal(available)
    if (paths.length === 0) return begin(action, [], yes, nextTicket())
    return inspect(action, paths, yes)
  }

  function begin(action, entries, yes, ticket) {
    var target = plan(action, entries, service.rootPath, null, yes, ticket)
    var error = gate(action, target.context, yes, Array.isArray(entries) ? entries.length : 0)
    if (error !== "") return refusal(error)
    dispatch(action, target)
    return { ok: true, request_id: target.ticket, key: action.key, context: target.context }
  }

  function inspect(action, paths, yes) {
    var parameters = []
    for (var i = 0; i < paths.length; i++) parameters.push("--path", String(paths[i]))
    var ticket = nextTicket()
    claim(action.key, ticket)
    service.backendRequest("stat-batch", parameters, generation, function(response) {
      if (controller.running[action.key] !== ticket) return
      controller.claim(action.key, "")
      controller.afterStat(action, ticket, response, yes)
    })
    return { ok: true, request_id: ticket, key: action.key, context: "pending" }
  }

  function afterStat(action, ticket, response, yes) {
    var payload = response && typeof response === "object" ? response : ({})
    var entries = Array.isArray(payload.entries) ? payload.entries : []
    if (payload.ok !== true || entries.length === 0) {
      store(ticket, refusal(ActionRows.firstLine(payload.error, noticeLength)
        || "could not inspect the given paths"))
      return
    }
    var answer = begin(action, entries, yes, ticket)
    if (answer.ok !== true) store(ticket, answer)
  }
  Component.onCompleted: refresh()
  onRegistryChanged: refresh()
  onCatalogProvidersChanged: refresh()
  property Connections registryLink: Connections {
    target: controller.registry
    ignoreUnknownSignals: true
    function onPluginsChanged() { controller.refresh() }
  }

  property Connections modulesLink: Connections {
    target: controller.service && controller.service.bladeHost
      ? controller.service.bladeHost.registry : null
    ignoreUnknownSignals: true
    function onRegistryChanged() { controller.refresh() }
  }
}
