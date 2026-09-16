.pragma library
.import "PathText.js" as PathText

function isFolder(tree, item) {
  return tree.expandableItems && !!item && item.kind !== "bin" && tree.expansionKey(item) !== ""
}

function expanded(tree, item) {
  return isFolder(tree, item) && tree.expandedFolders[tree.expansionKey(item)] === true
}

function children(tree, item) {
  var value = tree.childrenFor(item)
  return tree.childrenRevision >= 0 && Array.isArray(value) ? value : []
}

function toggle(tree, item) {
  var path = tree.expansionKey(item)
  if (!isFolder(tree, item)) return false
  var next = Object.assign({}, tree.expandedFolders)
  var opened = !expanded(tree, item)
  if (opened) next[path] = true
  else delete next[path]
  tree.expandedFolders = next
  tree.folderToggled(item, opened)
  return true
}

function prune(tree) {
  if (!tree.loadFolderChildren) return
  var roots = tree.items.filter(function(item) { return isFolder(tree, item) }).map(function(item) { return tree.itemPath(item) })
  var next = ({})
  for (var path of Object.keys(tree.expandedFolders))
    if (tree.expandedFolders[path] && roots.some(function(root) { return PathText.within(path, root) })) next[path] = true
  tree.expandedFolders = next
}

function appendRows(tree, result, item, depth, child) {
  result.push({ kind: "leaf", label: "", depth: depth, key: "", item: item, child: child })
  if (!expanded(tree, item)) return
  var nested = children(tree, item)
  for (var i = 0; i < nested.length; i++) appendRows(tree, result, nested[i], depth + 1, true)
}

function activate(tree, item) {
  if (item && item.kind === "bin") tree.actionRequested(item)
  else if (isFolder(tree, item)) toggle(tree, item)
  else tree.activated(item)
}

function expand(tree) {
  var row = tree.rowAt(tree.currentIndex)
  if (!row) return
  if (row.kind === "group" && tree.isCollapsed(row.key)) tree.toggleGroup(row.key)
  else if (row.kind === "leaf" && isFolder(tree, row.item) && !expanded(tree, row.item)) toggle(tree, row.item)
}

function collapse(tree) {
  var row = tree.rowAt(tree.currentIndex)
  if (!row) return
  if (row.kind === "group" && !tree.isCollapsed(row.key)) {
    tree.toggleGroup(row.key)
  } else if (row.kind === "leaf" && expanded(tree, row.item)) {
    toggle(tree, row.item)
  }
}

function parent(tree) {
  var row = tree.rowAt(tree.currentIndex)
  if (!row) return
  for (var i = tree.currentIndex - 1; i >= 0; i--) {
    if ((tree.rows[i].kind === "leaf" && tree.rows[i].depth < row.depth && isFolder(tree, tree.rows[i].item))
        || (tree.rows[i].kind === "group" && tree.rows[i].depth < row.depth)) {
      tree.currentIndex = i
      return
    }
  }
}

function branchScope(tree, all) {
  if (all) return { all: true, depth: -1, groups: [], path: "" }
  var row = tree.rowAt(tree.currentIndex)
  if (!row || (row.kind !== "group" && !isFolder(tree, row.item))) return null
  return { key: tree.rowKey(row), depth: row.depth,
    groups: row.kind === "group" ? row.path.slice() : null,
    path: row.kind === "leaf" ? tree.expansionKey(row.item) : "" }
}

function nextBranchEntry(tree, scope) {
  if (!scope) return null
  var start = scope.all ? 0 : tree.rows.findIndex(function(row) { return tree.rowKey(row) === scope.key })
  if (start < 0) return null
  for (var i = start; i < tree.rows.length; i++) {
    var row = tree.rows[i]
    if (!scope.all && i > start && row.depth <= scope.depth) break
    if (row.kind === "group" && tree.isCollapsed(row.key))
      return { group: true, key: row.key, depth: row.depth - scope.depth }
    if (row.kind !== "leaf" || !isFolder(tree, row.item)) continue
    if ((scope.all || i > start) && row.child && tree.isLinked(row.item)) {
      while (i + 1 < tree.rows.length && tree.rows[i + 1].depth > row.depth) i++
      continue
    }
    if (!expanded(tree, row.item)) return { item: row.item, depth: row.depth - scope.depth }
  }
  return null
}

function groupWithin(path, parent) {
  return path.length >= parent.length && parent.every(function(part, index) { return path[index] === part })
}

function collapseBranch(tree, scope) {
  var collapsed = Object.assign({}, tree.collapsed)
  var roots = scope.path ? [scope.path] : []
  if (scope.groups) {
    for (var item of tree.visibleItems) {
      var groups = tree.groupsFor(item) || []
      if (!groupWithin(groups, scope.groups)) continue
      for (var depth = Math.max(1, scope.groups.length); depth <= groups.length; depth++)
        collapsed[tree.groupKey(groups.slice(0, depth))] = true
      if (isFolder(tree, item)) roots.push(tree.expansionKey(item))
    }
  }
  var folders = ({})
  for (var path of Object.keys(tree.expandedFolders)) {
    if (!scope.all && !roots.some(function(root) { return path === root || PathText.within(path, root) })) folders[path] = tree.expandedFolders[path]
  }
  tree.expandedFolders = folders
  tree.collapsed = collapsed
}
