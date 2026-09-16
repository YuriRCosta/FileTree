import QtQuick
import QtTest
import "../../lib/ArtifactTreeFolders.js" as ArtifactTreeFolders
import "../../lib/FileIcons.js" as FileIcons

TestCase {
  name: "FileIconsSecurity"

  function test_folder_expansions_leave_no_previous_project_or_collapsed_keys() {
    var item = { path: "file:///project/%FF", isDir: true }
    var tree = { loadFolderChildren: true, expandableItems: true, items: [item],
      expandedFolders: ({ "file:///project/%FF": true, "file:///project/%FF/child": true,
        "/previous/skill": true, "/project/\uFFFD": true, "file:///project/%FF/collapsed": false }),
      itemPath: function(item) { return item.path }, expansionKey: function(item) { return item.path },
      folderToggled: function(item, open) { compare(open, false) } }
    ArtifactTreeFolders.prune(tree)
    compare(Object.keys(tree.expandedFolders).sort(), ["file:///project/%FF", "file:///project/%FF/child"])
    ArtifactTreeFolders.toggle(tree, item)
    verify(!Object.prototype.hasOwnProperty.call(tree.expandedFolders, item.path))
    tree.items = []
    ArtifactTreeFolders.prune(tree)
    compare(Object.keys(tree.expandedFolders).length, 0)
  }

  function test_theme_names_are_preserved() {
    compare(FileIcons.safeThemeIconName("org.gnome.Nautilus"), "org.gnome.Nautilus")
    compare(FileIcons.safeThemeIconName("nvim-nightly_2"), "nvim-nightly_2")
  }

  function test_paths_and_urls_are_rejected() {
    compare(FileIcons.safeThemeIconName("/tmp/untrusted.png"), "")
    compare(FileIcons.safeThemeIconName("file:///tmp/untrusted.png"), "")
    compare(FileIcons.safeThemeIconName("image://provider/untrusted"), "")
    compare(FileIcons.safeThemeIconName("../untrusted"), "")
    compare(FileIcons.safeThemeIconName("folder/untrusted"), "")
  }

  function test_names_are_bounded() {
    compare(FileIcons.safeThemeIconName("a".repeat(256)), "a".repeat(256))
    compare(FileIcons.safeThemeIconName("a".repeat(257)), "")
  }

  function test_symlink_icon_wins_over_file_and_directory_type() {
    compare(FileIcons.fileIcon("SKILL.md", true), "")
    compare(FileIcons.entryIcon("SKILL.md", false, true, false, false), "")
    compare(FileIcons.entryIcon("skill", true, true, false, false), "")
    compare(FileIcons.entryIcon("skill", true, false, false, false), FileIcons.folderIcon(false))
  }

  function test_folder_rows_recurse_through_directory_symlinks() {
    var root = { name: "skill", path: "/skills/one", isDir: true }
    var link = { name: "assets", path: "/skills/one/assets", isDir: true, isSymlink: true }
    var file = { name: "image.png", path: "/skills/one/assets/image.png", isDir: false }
    var tree = {
      expandableItems: true,
      expandedFolders: ({}),
      childrenRevision: 0,
      itemPath: function(item) { return item.path },
      expansionKey: function(item) { return item.isDir ? item.path : "" },
      childrenFor: function(item) { return item === root ? [link] : [file] },
      folderToggled: function(item, expanded) {}
    }
    var rows = []
    ArtifactTreeFolders.appendRows(tree, rows, root, 0, false)
    compare(rows.length, 1)
    verify(ArtifactTreeFolders.toggle(tree, root))
    rows = []
    ArtifactTreeFolders.appendRows(tree, rows, root, 0, false)
    compare(rows.length, 2)
    verify(ArtifactTreeFolders.isFolder(tree, link))
    verify(ArtifactTreeFolders.toggle(tree, link))
    rows = []
    ArtifactTreeFolders.appendRows(tree, rows, root, 0, false)
    compare(rows.length, 3)
    compare(FileIcons.entryIcon("assets", true, true, true, false), "")
  }

  function test_open_fold_is_idempotent_and_keeps_the_cursor() {
    var folder = { path: "/skills/one", isDir: true }
    var nested = { path: "/skills/one/SKILL.md", isDir: false }
    var rows = [{ kind: "leaf", item: folder, depth: 0 }, { kind: "leaf", item: nested, depth: 1 }]
    var tree = {
      currentIndex: 0, expandableItems: true, expandedFolders: ({ "/skills/one": true }),
      rowAt: function(index) { return rows[index] || null },
      itemPath: function(item) { return item.path },
      expansionKey: function(item) { return item.isDir ? item.path : "" },
      move: function(delta) { this.currentIndex += delta },
      activated: function() { fail("must not activate a folder") }
    }
    ArtifactTreeFolders.expand(tree)
    compare(tree.currentIndex, 0)
    verify(tree.expandedFolders[folder.path])
    tree.currentIndex = 0
    rows = rows.slice(0, 1)
    ArtifactTreeFolders.expand(tree)
    compare(tree.currentIndex, 0)
  }

  function test_binned_rows_restore_without_opening_a_replacement_at_the_old_path() {
    var item = { kind: "bin", path: "/skills/one", isDir: true }
    var requested = 0
    var tree = {
      currentIndex: 0, expandableItems: true,
      rowAt: function() { return { kind: "leaf", item: item, depth: 0 } },
      actionRequested: function(entry) { compare(entry, item); requested++ },
      activated: function() { fail("must not open the historical path") }
    }
    verify(!ArtifactTreeFolders.isFolder(tree, item))
    ArtifactTreeFolders.activate(tree, item)
    ArtifactTreeFolders.expand(tree)
    compare(requested, 1)
  }
}
