.pragma library

var CAPACITY_FALLBACK_BLUE = "#7aa2f7"

function folderReady(model, path) {
  var root = model.count ? model.get(0) : null
  return !!root && (path === undefined || root.path === path) && root.expanded && root.loaded && !root.loading && !root.error
}

function folderCount(model, path, structureRevision, rowsRevision) {
  if (!folderReady(model, path)) {
    model.folderCountCache = null
    return { loaded: 0, total: 0, known: false }
  }
  var version = structureRevision === undefined ? "" : JSON.stringify([path, model.count, structureRevision, rowsRevision])
  var cached = version ? model.folderCountCache : null
  if (cached && cached.version === version) return cached.value
  var loaded = 0, total = 0
  for (var i = 0; i < model.count; i++) {
    var row = model.get(i)
    if (row.depth !== 1) continue
    if (row.kind === "More") total = Math.max(total, Number(row.windowTotal) || 0)
    else if (!row.gitDeleted) loaded++
  }
  var value = { loaded: loaded, total: Math.max(loaded, total), known: true }
  if (version) model.folderCountCache = { version: version, value: value }
  return value
}
