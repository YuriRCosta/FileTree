.pragma library

var counts = new WeakMap()

function folderReady(model, path) {
  var root = model.count ? model.get(0) : null
  return !!root && (path === undefined || root.path === path) && root.expanded && root.loaded && !root.loading && !root.error
}

function folderCount(model, path, structureRevision, rowsRevision) {
  if (!folderReady(model, path)) {
    counts.delete(model)
    return { loaded: 0, total: 0, known: false }
  }
  var version = structureRevision === undefined ? "" : JSON.stringify([path, model.count, structureRevision, rowsRevision])
  var cached = version ? counts.get(model) : null
  if (cached && cached.version === version) return cached.value
  var loaded = 0, total = 0
  for (var i = 0; i < model.count; i++) {
    var row = model.get(i)
    if (row.depth !== 1) continue
    if (row.kind === "More") total = Math.max(total, Number(row.windowTotal) || 0)
    else if (!row.gitDeleted) loaded++
  }
  var value = { loaded: loaded, total: Math.max(loaded, total), known: true }
  if (version) counts.set(model, { version: version, value: value })
  return value
}
