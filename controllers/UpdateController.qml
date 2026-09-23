import QtQuick

Item {
  id: controller

  required property var service
  property var host: null

  readonly property int checkIntervalMs: 6 * 60 * 60 * 1000
  readonly property string coreId: service && service.manifest && service.manifest.id ? String(service.manifest.id) : "data-goblin.fileblade"
  readonly property bool checksEnabled: !host || !host.config || host.config.checkUpdates !== false

  property bool busy: false
  property var report: ({})
  property string error: ""
  property int generation: 0
  property bool upToDateNotice: false
  readonly property int upToDateNoticeMs: 10000

  readonly property var repositories: report && Array.isArray(report.repositories) ? report.repositories : []
  readonly property var core: {
    for (var i = 0; i < repositories.length; i++) if (repositories[i].id === coreId) return repositories[i]
    return null
  }
  readonly property bool coreUpdatable: !!core && core.updatable === true
  readonly property bool backendStale: !!core && core.backend_stale === true
  readonly property bool available: coreUpdatable
  readonly property string chipText: available ? "Update available" : (backendStale ? "Backend update needed" : (upToDateNotice ? "FileTree is up to date!" : ""))
  readonly property bool chipVisible: available || backendStale || upToDateNotice

  Timer {
    id: upToDateTimer
    interval: controller.upToDateNoticeMs
    onTriggered: controller.upToDateNotice = false
  }

  Connections {
    target: controller.service
    function onStateReadyChanged() { if (controller.service.stateReady) controller.checkIfStale() }
  }

  function coreNotice() {
    var version = String(core.upstream_version || "")
    if (!version) return "An update for FileTree is available; its version could not be determined."
    if (core.version_change === "same") return "FileTree has updates available within version " + version + "."
    if (core.version_change === "older") return "FileTree's upstream changed to version " + version + " (installed: " + core.current_version + ")."
    return "Version " + version + " of FileTree is now available!"
  }

  function summaryLines() {
    var lines = []
    if (coreUpdatable) lines.push(coreNotice())
    if (core && !coreUpdatable && (core.dirty || core.ahead > 0)) lines.push("Skipped FileTree: " + (core.dirty ? "local changes" : "local commits ahead"))
    if (backendStale) lines.push("Backend binary is " + String(core.backend_version || "") + ", checkout is " + String(core.current_version || "") + ": update or reinstall FileTree")
    return lines
  }

  function dialogLines() {
    var lines = summaryLines()
    if (available) {
      lines.push("FileTree only checks for updates; it does not install them while running.")
      lines.push("Stop the shell before replacing plugin files; update with omarchy plugin update, then run omarchy restart shell. The backend is included.")
    } else if (backendStale) {
      lines.push("Update or reinstall FileTree, then run omarchy restart shell. Check FILEBLADE_BINARY if you use a custom backend.")
    }
    return lines
  }

  function repositorySpecs() {
    return host && host.pluginDir ? [coreId + "=" + host.pluginDir] : []
  }

  function specArguments(specs) {
    var arguments = ["--core", coreId]
    for (var i = 0; i < specs.length; i++) arguments.push("--repository", specs[i])
    return arguments
  }

  function checkIfStale() {
    if (!checksEnabled || busy) return
    var last = Number(service.updateCheckedAt) || 0
    if (Date.now() - last < checkIntervalMs) return
    check()
  }

  function check() {
    if (!service || busy) return
    var specs = repositorySpecs()
    if (specs.length === 0) return
    busy = true
    error = ""
    service.markUpdateChecked(Date.now())
    var requestGeneration = ++generation
    service.backendRequest("update-check", specArguments(specs), requestGeneration, function(response) {
      if (requestGeneration !== controller.generation) return
      controller.busy = false
      if (!response || response.ok !== true) {
        controller.error = response && response.error ? String(response.error) : "Update check failed"
        return
      }
      controller.report = response
      controller.upToDateNotice = !controller.available && !controller.backendStale && controller.repositories.length > 0 && controller.repositories.every(function(row) { return row.ok !== false && !row.dirty && !(row.ahead > 0) })
      if (controller.upToDateNotice) upToDateTimer.restart()
    }, null, 240000)
  }
}
