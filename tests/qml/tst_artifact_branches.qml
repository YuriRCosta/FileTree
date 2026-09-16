import QtQuick
import QtTest
import "../../lib/ArtifactTreeFolders.js" as Folders

TestCase {
  name: "ArtifactBranches"
  function folder(path) { return { path: path, isDir: true } }
  function leaf(item, depth, child) { return { kind: "leaf", item: item, depth: depth, child: !!child } }
  function group(path) { return { kind: "group", key: JSON.stringify(path), path: path, depth: path.length - 1 } }
  function fixture(rows) {
    var tree = { rows: rows, currentIndex: 0, expandableItems: true, expandedFolders: ({}), collapsed: ({}), visibleItems: [],
      rowAt: function(index) { return tree.rows[index] || null },
      rowKey: function(row) { return row.kind === "group" ? row.key : row.item.path },
      groupKey: function(path) { return JSON.stringify(path) },
      groupsFor: function(item) { return item.groups || [] },
      itemPath: function(item) { return item.path },
      expansionKey: function(item) { return item.isDir ? item.path : "" },
      isCollapsed: function(key) { return tree.collapsed[key] === true },
      isLinked: function(item) { return !!item.isSymlink }
    }
    return tree
  }

  function test_branch_scope_is_captured_and_excludes_siblings() {
    var root = folder("/skills/one"), child = folder("/skills/one/child"), other = folder("/skills/two")
    var tree = fixture([leaf(root, 0), leaf(child, 1, true), leaf(other, 0)])
    var scope = Folders.branchScope(tree)
    tree.currentIndex = 2
    compare(Folders.nextBranchEntry(tree, scope).item, root)
    tree.expandedFolders[root.path] = true
    compare(Folders.nextBranchEntry(tree, scope).item, child)
    tree.expandedFolders[child.path] = true
    compare(Folders.nextBranchEntry(tree, scope), null)
  }

  function test_recursive_collapse_clears_hidden_descendants_with_byte_faithful_boundaries() {
    var tree = fixture([leaf(folder("file:///skills/%FF"), 0)])
    tree.expandedFolders = { "file:///skills/%FF": true, "file:///skills/%FF/hidden/child": true,
      "/skills/\uFFFD": true, "file:///skills/%FFsibling": true }
    Folders.collapseBranch(tree, Folders.branchScope(tree))
    compare(Object.keys(tree.expandedFolders).sort(), ["/skills/\uFFFD", "file:///skills/%FFsibling"].sort())
  }

  function test_groups_collapse_all_their_child_groups_and_folders_only() {
    var tree = fixture([group(["User"])])
    tree.visibleItems = [Object.assign(folder("/one"), { groups: ["User", "Skills"] }),
      Object.assign(folder("/two"), { groups: ["Project", "Skills"] })]
    tree.expandedFolders = { "/one": true, "/one/child": true, "/two": true }
    Folders.collapseBranch(tree, Folders.branchScope(tree))
    compare(tree.collapsed, { '["User"]': true, '["User","Skills"]': true })
    compare(tree.expandedFolders, { "/two": true })
    compare(Folders.nextBranchEntry(tree, Folders.branchScope(tree)).key, '["User"]')
    tree.collapsed['["User"]'] = false
    tree.rows = [group(["User"]), group(["User", "Skills"]), group(["Project"])]
    compare(Folders.nextBranchEntry(tree, Folders.branchScope(tree)).key, '["User","Skills"]')
  }

  function test_directory_symlink_descendants_are_skipped_but_explicit_targets_work() {
    var root = folder("/one"), link = Object.assign(folder("/one/link"), { isSymlink: true })
    var tree = fixture([leaf(root, 0), leaf(link, 1, true), leaf(folder("/one/link/child"), 2, true)])
    tree.expandedFolders[root.path] = true
    compare(Folders.nextBranchEntry(tree, Folders.branchScope(tree)), null)
    tree.currentIndex = 1
    compare(Folders.nextBranchEntry(tree, Folders.branchScope(tree)).item, link)
  }

  function test_files_and_historical_bin_rows_do_not_expand_or_activate() {
    for (var item of [{ path: "/one/file" }, { path: "/one", isDir: true, kind: "bin" }])
      compare(Folders.branchScope(fixture([leaf(item, 0)])), null)
    compare(Folders.branchScope(fixture([])), null)
  }

  function test_whole_tree_expansion_reaches_every_group_and_collapse_clears_descendants() {
    var tree = fixture([group(["User"]), group(["Project"])])
    tree.visibleItems = [Object.assign(folder("/one"), { groups: ["User", "Skills"] }),
      Object.assign(folder("/two"), { groups: ["Project", "Skills"] })]
    tree.expandedFolders = { "/one": true, "/one/hidden": true, "/two": true }
    var scope = Folders.branchScope(tree, true)
    Folders.collapseBranch(tree, scope)
    compare(tree.expandedFolders, ({}))
    compare(tree.collapsed, { '["User"]': true, '["User","Skills"]': true,
      '["Project"]': true, '["Project","Skills"]': true })
    compare(Folders.nextBranchEntry(tree, scope).key, '["User"]')
    tree.collapsed['["User"]'] = false
    compare(Folders.nextBranchEntry(tree, scope).key, '["Project"]')
  }

  function test_parent_navigation_does_not_change_folds() {
    var root = folder("/one"), child = folder("/one/child")
    var tree = fixture([leaf(root, 0), leaf(child, 1, true)])
    tree.currentIndex = 1
    tree.expandedFolders = { "/one": true, "/one/child": true }
    Folders.parent(tree)
    compare(tree.currentIndex, 0)
    compare(tree.expandedFolders, { "/one": true, "/one/child": true })
  }

  function test_definitions_sharing_one_file_fold_by_their_own_key() {
    var first = { id: "first", source: { path: "~/.claude.json" }, observed: [{ name: "navigate" }] }
    var second = { id: "second", source: { path: "~/.claude.json" }, observed: [{ name: "search" }] }
    var tree = fixture([leaf(first, 2), leaf(second, 2)])
    tree.itemPath = function(item) { return item.source.path }
    tree.expansionKey = function(item) { return item.observed ? item.id : "" }
    tree.childrenFor = function(item) { return item.observed }
    tree.childrenRevision = 0
    tree.folderToggled = function() {}
    verify(Folders.toggle(tree, first))
    compare(tree.expandedFolders, { first: true })
    var rows = []
    for (var item of [first, second]) Folders.appendRows(tree, rows, item, 2, false)
    compare(rows.map(function(row) { return row.item.name || row.item.id }), ["first", "navigate", "second"])
    verify(rows[1].child)
    verify(!Folders.isFolder(tree, rows[1].item))
    Folders.collapseBranch(tree, Folders.branchScope(tree))
    compare(tree.expandedFolders, ({}))
  }

  function test_closing_a_fold_does_not_navigate_to_its_parent() {
    var root = folder("/one"), child = folder("/one/child")
    var tree = fixture([leaf(root, 0), leaf(child, 1, true)])
    tree.folderToggled = function() {}
    tree.currentIndex = 1
    tree.expandedFolders = { "/one": true, "/one/child": true }
    Folders.collapse(tree)
    compare(tree.expandedFolders, { "/one": true })
    Folders.collapse(tree)
    compare(tree.currentIndex, 1)
    compare(tree.expandedFolders, { "/one": true })
  }
}
