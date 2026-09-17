import QtQuick

Item {
  id: listing
  visible: false
  required property var service
  property string module: ""
  property bool active: true
  property var rows: []
  property string error: ""
  property int failures: 0
  property var request: null
  property int generation: 0
  property bool stopping: false
  readonly property bool ready: !stopping && active && !!service && module !== ""
  readonly property int maximumRetries: 5

  function cancel() {
    var previous = request
    request = null
    if (previous) previous.service.cancelBackendRequest(previous.id, previous.generation, true)
  }

  function suspend() {
    reload.stop()
    retry.stop()
    cancel()
  }

  function refresh() {
    suspend()
    if (ready) reload.restart()
  }

  function invalidate() {
    rows = []
    error = ""
    failures = 0
    refresh()
  }

  function start() {
    suspend()
    if (!ready) return
    var read = { id: "", generation: ++generation, service: service }
    request = read
    read.id = service.backendRequest("bin-list", ["--module", module], read.generation, function(response) {
      if (listing.request !== read) return
      listing.request = null
      listing.accept(response)
    })
  }

  function accept(response) {
    if (response && response.ok === true && Array.isArray(response.items)) {
      rows = response.items
      error = ""
      failures = 0
      return
    }
    error = String(response && (response.error || response.message) || "The bin could not be listed").slice(0, 200)
    if (!ready || failures >= maximumRetries) return
    failures++
    retry.interval = 1000 * Math.pow(2, failures - 1)
    retry.restart()
  }

  onModuleChanged: invalidate()
  onServiceChanged: invalidate()
  onActiveChanged: refresh()
  Component.onCompleted: refresh()
  Component.onDestruction: { stopping = true; suspend() }

  Timer { id: reload; interval: 0; onTriggered: listing.start() }
  Timer { id: retry; repeat: false; onTriggered: if (listing.ready && !listing.request) listing.start() }
}
