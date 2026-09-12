.pragma library

function folderCount(model) {
  var loaded = 0, total = 0
  for (var i = 0; i < model.count; i++) {
    var row = model.get(i)
    if (row.depth !== 1) continue
    if (row.kind === "More") total = Math.max(total, Number(row.windowTotal) || 0)
    else if (!row.gitDeleted) loaded++
  }
  return { loaded: loaded, total: Math.max(loaded, total) }
}
