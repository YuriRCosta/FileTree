import QtQuick
import qs.Commons
import "../../lib/Format.js" as Format

FocusScope {
  id: module

  property var context: null

  readonly property string title: "Branches"
  readonly property var shortcuts: [
    {
      title: "Branches",
      items: [
        { shortcut: "Enter", text: "Switch to branch, or open worktree" },
        { shortcut: "o", text: "Open the worktree a branch is checked out in" },
        { shortcut: "s", text: "Cycle sort" },
        { shortcut: "f", text: "Filter" },
        { shortcut: "Shift+R", text: "Rescan" }
      ]
    }
  ].concat(files && files.keybindings ? [files.keybindings.treeShortcuts] : [])
  readonly property var metricOptions: context ? context.metrics.options(["off", { key: "kind", label: "Kind" }, "status", "updated", { key: "author", label: "Author" }, "summary"]) : []
  readonly property var view: viewLoader.item
  readonly property var files: context ? context.service("files") : null
  readonly property string anchorPath: files ? String(files.contextPath || files.rootPath || "") : ""
  readonly property bool active: !!context && context.bladeOpen !== false && context.collapsed !== true
  readonly property color paneBackground: Qt.lighter(Color.background, 1.035)

  property var document: null
  property string loadError: ""
  property string switchError: ""
  property bool busy: false
  property bool switching: false
  property int generation: 0
  property string query: ""
  property bool caseSensitive: false
  property bool regex: false

  readonly property var branches: document && Array.isArray(document.branches) ? document.branches : []
  readonly property var worktrees: document && Array.isArray(document.worktrees) ? document.worktrees : []
  readonly property var items: buildItems(worktrees, branches)
  readonly property string error: loadError || switchError
  readonly property string status: {
    if (switching) return "Switching…"
    if (error !== "") return error
    if (busy && !document) return "Loading…"
    if (tree.item && tree.item.searching) return tree.item.visibleItems.length + " of " + items.length
    return branches.length + " branches, " + worktrees.length + " worktrees"
  }

  function takeFocus(part) {
    if (tree.item) tree.item.forceActiveFocus()
  }

  function refresh() {
    if (!files || !active || anchorPath === "") return
    var serial = ++generation
    busy = true
    files.backendRequest("git-places", ["--path", anchorPath], serial, function(response) {
      if (serial !== module.generation) return
      module.busy = false
      if (!response || response.ok !== true) {
        module.loadError = response && response.error ? String(response.error) : "branch list is unavailable"
        module.document = null
        return
      }
      module.loadError = ""
      module.document = response
    })
  }

  function switchTo(entry) {
    if (!files || switching || !entry || entry.current) return
    var root = document && document.root ? String(document.root) : anchorPath
    switching = true
    files.backendRequest("git-switch", ["--path", root, "--branch", String(entry.name)], 0, function(response) {
      module.switching = false
      if (!response || response.ok !== true) {
        module.switchError = response && response.error ? String(response.error) : "branch switch failed"
        return
      }
      module.switchError = ""
      if (typeof files.requestVisibleGitMetadataRefresh === "function") files.requestVisibleGitMetadataRefresh("branch-switch")
      if (typeof files.refreshTree === "function") files.refreshTree()
      module.refresh()
    })
  }

  function openWorktree(path) {
    if (files && path) files.navigateToLocation(String(path), context.screen, "browse")
  }

  function activate(entry) {
    if (!entry) return
    if (entry.kind === "worktree") openWorktree(entry.path)
    else switchTo(entry)
  }

  function leafName(path) {
    var text = String(path || "").replace(/\/+$/, "")
    return text.slice(text.lastIndexOf("/") + 1)
  }

  function worktreeStatus(entry) {
    var parts = []
    if (entry.conflicted > 0) parts.push(entry.conflicted + " conflicts")
    if (entry.staged > 0) parts.push(entry.staged + " staged")
    if (entry.unstaged > 0) parts.push(entry.unstaged + " changed")
    if (entry.untracked > 0) parts.push(entry.untracked + " untracked")
    var text = parts.length > 0 ? parts.join(", ") : "clean"
    return entry.locked ? text + ", locked" : text
  }

  function branchStatus(entry, checkouts) {
    if (entry.worktree && checkouts[entry.worktree]) return worktreeStatus(checkouts[entry.worktree])
    if (entry.kind === "remote") return "remote"
    if (entry.gone) return "gone"
    if (!entry.upstream) return "no upstream"
    var parts = []
    if (entry.ahead > 0) parts.push("↑" + entry.ahead)
    if (entry.behind > 0) parts.push("↓" + entry.behind)
    return parts.length > 0 ? parts.join(" ") : "in sync"
  }

  function updatedText(at) {
    var seconds = Number(at)
    return isFinite(seconds) && seconds > 0 ? Format.isoDate(new Date(seconds * 1000)) : ""
  }

  function buildItems(worktreeRows, branchRows) {
    var checkouts = ({})
    var result = []
    for (var w = 0; w < worktreeRows.length; w++) {
      var tree = worktreeRows[w]
      var path = String(tree.path || "")
      checkouts[path] = tree
      result.push({ id: "worktree:" + path, kind: "worktree", name: leafName(path), path: path,
                    detail: tree.branch ? String(tree.branch) : "detached at " + String(tree.head || ""),
                    current: tree.current === true, groups: ["Worktrees"],
                    metrics: { kind: "worktree", status: worktreeStatus(tree), updated: updatedText(tree.at), author: "" } })
    }
    for (var b = 0; b < branchRows.length; b++) {
      var branch = branchRows[b]
      result.push({ id: "branch:" + String(branch.name), kind: String(branch.kind || "local"), name: String(branch.name || ""),
                    detail: String(branch.subject || ""), current: branch.current === true, worktree: String(branch.worktree || ""),
                    remote: String(branch.remote || ""), upstream: String(branch.upstream || ""), author: String(branch.author || ""),
                    groups: branch.kind === "remote" ? ["Branches", "Remote"] : ["Branches"],
                    metrics: { kind: String(branch.kind || "local"), status: branchStatus(branch, checkouts),
                               updated: updatedText(branch.at), author: String(branch.author || "") } })
    }
    return result
  }

  function glyphFor(entry) {
    if (entry.current) return "󰄬"
    if (entry.kind === "worktree") return "󰉖"
    return entry.kind === "remote" ? "󰅡" : "󰘬"
  }

  function searchText(entry) {
    return [entry.name, entry.detail, entry.author, entry.upstream, entry.kind].join(" ")
  }

  function handleKey(event) {
    if (event.modifiers !== Qt.NoModifier || event.key !== Qt.Key_O || !tree.item) return false
    var row = tree.item.rowAt(tree.item.currentIndex)
    if (!row || row.kind !== "leaf" || row.item.kind === "worktree" || !row.item.worktree) return false
    openWorktree(row.item.worktree)
    return true
  }

  onActiveChanged: refresh()
  onAnchorPathChanged: refresh()
  Component.onCompleted: refresh()

  Connections {
    target: module.files
    ignoreUnknownSignals: true
    function onGitMetadataRefreshCountChanged() { module.refresh() }
  }

  Loader {
    id: viewLoader
    source: module.context ? module.context.ui.url("PaneView") : ""
    onLoaded: {
      item.defaultMetric = "status"
      item.options = module.metricOptions
      item.context = Qt.binding(function() { return module.context })
    }
  }

  Rectangle {
    anchors.fill: parent
    color: module.paneBackground
  }

  Loader {
    id: header
    anchors.top: parent.top
    anchors.left: parent.left
    anchors.right: parent.right
    source: module.context ? module.context.ui.url("PaneHeader") : ""
    onLoaded: {
      item.context = module.context
      item.title = "BRANCHES"
      item.tabIndex = Qt.binding(function() { return module.context.tabIndex })
      item.reservedLeft = Qt.binding(function() { return module.context.cornerReserveLeft })
      item.reservedRight = Qt.binding(function() { return module.context.cornerReserveRight })
      item.highlighted = Qt.binding(function() { return module.activeFocus })
      item.view = Qt.binding(function() { return module.view })
      item.status = Qt.binding(function() { return module.status })
      item.statusColor = Qt.binding(function() { return module.error !== "" ? Color.urgent : Color.muted })
    }
  }

  Loader {
    id: search
    anchors.top: header.bottom
    anchors.topMargin: height > 0 ? Style.space(6) : 0
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.leftMargin: Style.space(7)
    anchors.rightMargin: Style.space(7)
    active: module.active
    height: item && item.visible ? Style.space(32) : 0
    source: module.context ? module.context.ui.url("PaneSearchField") : ""
    onLoaded: {
      item.context = Qt.binding(function() { return module.context })
      item.prompt = "Filter branches…"
      item.text = Qt.binding(function() { return module.query })
      item.showOptions = true
      item.caseSensitive = module.caseSensitive
      item.regex = module.regex
      item.optionsToggled.connect(function(nextCase, nextRegex) {
        module.caseSensitive = nextCase
        module.regex = nextRegex
      })
      item.textChanged.connect(function() { module.query = item.text })
      item.dismissed.connect(function() { module.closeSearch() })
      item.advanced.connect(function() { module.takeFocus("") })
    }
  }

  function openSearch() {
    if (search.item) search.item.reveal()
  }

  function closeSearch() {
    query = ""
    takeFocus("")
  }

  function openFilter() {
    if (header.item) header.item.openFilter()
  }

  Loader {
    id: tree
    anchors.top: search.bottom
    anchors.topMargin: Style.space(4)
    anchors.bottom: parent.bottom
    anchors.left: parent.left
    anchors.right: parent.right
    active: module.active
    visible: active
    source: module.context ? module.context.ui.url("ArtifactTree") : ""
    onLoaded: {
      item.context = Qt.binding(function() { return module.context })
      item.items = Qt.binding(function() { return module.items })
      item.query = Qt.binding(function() { return module.query })
      item.caseSensitive = Qt.binding(function() { return module.caseSensitive })
      item.regex = Qt.binding(function() { return module.regex })
      item.view = Qt.binding(function() { return module.view })
      item.surfaceColor = Qt.binding(function() { return module.paneBackground })
      item.fileActionsFor = function(entry) { return false }
      item.groupsFor = function(entry) { return entry.groups }
      item.leafGlyph = function(entry) { return module.glyphFor(entry) }
      item.rowMark = function(entry) { return "" }
      item.searchText = function(entry) { return module.searchText(entry) }
      item.filterKeys = ["kind", "remote"]
      item.searchFields = function(entry) { return ({ kind: entry.kind, remote: entry.remote }) }
      item.keyHandler = function(event) { return module.handleKey(event) }
      item.activated.connect(function(entry) { module.activate(entry) })
      item.revealed.connect(function(entry) { if (entry.worktree) module.openWorktree(entry.worktree) })
      item.searchRequested.connect(function() { module.openSearch() })
      item.filterRequested.connect(function() { module.openFilter() })
      item.focusNextRequested.connect(function() { module.context.focusNext() })
      item.focusPreviousRequested.connect(function() { module.context.focusPrevious() })
      item.dismissRequested.connect(function() { module.context.closeBlade() })
    }
  }

  Keys.onPressed: function(event) {
    if (event.key !== Qt.Key_R || !(event.modifiers & Qt.ShiftModifier) || (event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))) return
    module.refresh()
    event.accepted = true
  }

  Text {
    textFormat: Text.PlainText
    anchors.centerIn: parent
    width: Math.max(0, parent.width - Style.space(40))
    visible: module.active && (module.items.length === 0 || module.loadError !== "")
    horizontalAlignment: Text.AlignHCenter
    wrapMode: Text.WordWrap
    text: module.loadError !== "" ? module.loadError : (module.busy ? "Loading branches…" : (module.query ? "No match" : "No branches found"))
    color: module.loadError !== "" ? Color.urgent : Color.muted
    font.family: Style.font.family
    font.pixelSize: Style.font.body
  }
}
