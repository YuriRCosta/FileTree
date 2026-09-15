import QtQuick

Item {
  id: inventory
  visible: false

  property var files: null
  property string providerId: ""
  property string providerRoot: ""
  property string helperId: "inventory"
  property string itemsKey: "items"
  property string healthBasis: ""
  property bool exactProject: true
  property var scanArguments: []
  property int maximumItems: 1000
  property var observers: []
  readonly property string anchorPath: files ? String(files.contextPath !== undefined
    ? files.contextPath : (files.projectRoot || files.selectedPath || files.rootPath || "")) : ""
  readonly property var projectArguments: exactProject && files && (files.contextPath !== undefined || files.projectRoot) ? ["--exact"] : []
  readonly property bool ready: !stopping && observers.length > 0 && !!files && providerId !== "" && providerRoot !== ""
  readonly property var lanes: [projectLane, userLane]
  property var items: []
  property string itemsFingerprint: ""
  property bool overflow: false
  readonly property string projectRoot: projectLane.projectRoot
  readonly property string loadError: projectLane.loadError || userLane.loadError
  readonly property string watchError: projectLane.watchError || userLane.watchError
  readonly property bool truncated: overflow || projectLane.truncated || userLane.truncated
  readonly property bool busy: projectLane.busy || userLane.busy
  property string applyError: ""
  readonly property bool applying: mutation !== null
  property int generation: 0
  property var mutation: null
  property bool stopping: false

  signal mutationFinished(string method, var response, string project)

  InventoryLane { id: projectLane; owner: inventory; scope: "project"; onItemsChanged: inventory.publish() }
  InventoryLane { id: userLane; owner: inventory; scope: "user"; onItemsChanged: inventory.publish() }

  function attach(context) {
    if (!context || observers.indexOf(context) >= 0) return
    if (!files) files = context.service("files")
    observers = observers.concat([context])
  }

  function detach(context) {
    observers = observers.filter(function(value) { return value !== context })
  }

  function argumentsFor(method, arguments) {
    return ["--provider", providerId, "--plugin-dir", ["fileblade.core.skills", "fileblade.core.memory", "fileblade.core.hooks", "fileblade.core.mcp"].indexOf(providerId) >= 0 ? "" : providerRoot,
            "--helper", helperId, "--method", method, "--arguments", JSON.stringify(arguments)]
  }

  function refresh(retryWatch) {
    projectLane.refresh(retryWatch)
    userLane.refresh(retryWatch)
  }

  function startScan() {
    projectLane.startScan()
    userLane.startScan()
  }

  function publish() {
    var rows = projectLane.items.concat(userLane.items)
    overflow = rows.length > maximumItems
    rows = rows.slice(0, maximumItems)
    var fingerprint = JSON.stringify(rows)
    if (fingerprint === itemsFingerprint) return
    itemsFingerprint = fingerprint
    items = rows
  }

  function boundedMetrics(raw) {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) return ({})
    var result = ({})
    for (var date of ["updated", "created"]) result[date] = String(raw[date] || "").slice(0, 32)
    for (var count of ["bytes", "characters", "words", "tokens", "fileTokens",
                       "uses", "usesAgent", "usesUser", "usesScheduled", "failed"]) {
      var value = raw[count], number = Number(value)
      result[count] = value === null || value === undefined || !isFinite(number) ? null : Math.max(0, number)
    }
    return result
  }

  function failureMessage(response) {
    var message = response && (response.error || response.message)
    var results = response && Array.isArray(response.results) ? response.results : []
    for (var i = 0; !message && i < results.length; i++)
      if (results[i] && results[i].ok === false) message = results[i].message
    return String(message || "Change refused").slice(0, 200)
  }

  function mutate(method, arguments, input, callback) {
    if (!ready || applying || !Array.isArray(arguments)) return false
    if (["fileblade.core.skills", "fileblade.core.memory", "data-goblin.fileblade-skills", "data-goblin.fileblade-memory"].indexOf(providerId) >= 0 && files.agentManagementEnabled !== true) {
      applyError = "Enable Manage agent files in General settings to change Skills or Memory"
      return false
    }
    applyError = ""
    var request = { id: "", generation: generation, files: files, project: anchorPath, method: method }
    mutation = request
    generation++
    for (var lane of lanes) {
      lane.generation++
      lane.suspendScan()
    }
    request.id = files.backendRequest("helper-write", argumentsFor(method, arguments.slice()), request.generation, function(response) {
      if (inventory.stopping || inventory.mutation !== request) return
      inventory.mutation = null
      if (response && response.ok === true && response.schemaVersion !== 1)
        response = { ok: false, error: "Change returned no readable payload" }
      if (request.project === inventory.anchorPath && (!response || response.ok !== true))
        inventory.applyError = inventory.failureMessage(response)
      inventory.refresh()
      inventory.mutationFinished(request.method, response, request.project)
      if (typeof callback === "function") callback(response)
    }, null, 35000, { input: input === undefined ? "" : String(input), untimed: true })
    return true
  }

  onAnchorPathChanged: {
    applyError = ""
    projectLane.invalidate()
  }
  onReadyChanged: {
    if (ready) refresh()
    else for (var lane of lanes) lane.suspend()
  }
  Component.onDestruction: {
    stopping = true
    for (var lane of lanes) lane.shutdown()
    if (mutation) mutation.files.cancelBackendRequest(mutation.id, mutation.generation, true)
  }
}
