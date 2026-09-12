import QtQuick
import qs.Commons
import "../../panes"

FocusScope {
  id: module

  required property var context

  readonly property var controller: context.service("files")
  readonly property string title: "FileBlade"
  readonly property Component settings: filesSettings
  function keys(action) { return controller.keybindings.label(action) }
  readonly property var shortcuts: [
    {
      title: "FileBlade",
      items: [
        { shortcut: keys("next") + " / " + keys("previous"), text: "Move down / up" },
        { shortcut: keys("up"), text: "Parent folder" },
        { shortcut: keys("open"), text: "Open file / enter folder" },
        { shortcut: keys("expand") + " / " + keys("collapse"), text: "Expand / collapse folder" },
        { shortcut: keys("expand-recursive"), text: "Expand selected subtree" },
        { shortcut: keys("collapse-recursive"), text: "Collapse selected subtree" },
        { shortcut: keys("expand-all") + " / " + keys("collapse-all"), text: "Expand / collapse whole tree" },
        { shortcut: keys("first") + " / " + keys("last"), text: "First / last" },
        { shortcut: keys("page-next"), text: "Page down" },
        { shortcut: keys("page-previous"), text: "Page up" },
        { shortcut: keys("activate"), text: "Open file / toggle folder expansion" },
        { shortcut: "Shift+Enter", text: "Open with" },
        { shortcut: "e", text: "Edit in LazyVim" },
        { shortcut: keys("search"), text: "Search" },
        { shortcut: keys("deep"), text: "Deep fuzzy search (whole root)" },
        { shortcut: keys("help"), text: "This list" },
        { shortcut: keys("layout"), text: "Deep results as a tree" },
        { shortcut: "↑/↓ in search", text: "Earlier searches" },
        { shortcut: keys("quicknav"), text: "Quick nav (folders)" },
        { shortcut: keys("picker"), text: "Picker (> actions, ~ recent, ? contents)" },
        { shortcut: "Ctrl+L", text: "Location" },
        { shortcut: "Backspace  Alt+←/→", text: "Back / forward" },
        { shortcut: "Alt+↑/Home", text: "Up / home" },
        { shortcut: ".  Shift+H  Ctrl+H", text: "Hidden files" },
        { shortcut: "Shift+R", text: "Refresh" },
        { shortcut: "v", text: "Visual select" },
        { shortcut: "Ctrl+Space", text: "Toggle selection" },
        { shortcut: "Ctrl+A", text: "Select all" },
        { shortcut: "a  Ctrl+N", text: "New file" },
        { shortcut: "Ctrl+Shift+N", text: "New folder" },
        { shortcut: "r  F2", text: "Rename" },
        { shortcut: "d  Delete", text: "Trash" },
        { shortcut: "y  Ctrl+C", text: "Copy" },
        { shortcut: "x  Ctrl+X", text: "Cut" },
        { shortcut: "p  Ctrl+V", text: "Paste" },
        { shortcut: "m  Menu", text: "Actions" },
        { shortcut: "u  Ctrl+Z", text: "Undo" },
        { shortcut: "Ctrl+Shift+Z  Ctrl+Y/R", text: "Redo" },
        { shortcut: "Shift+U", text: "Skip a refused undo" },
        { shortcut: "Esc", text: "Dismiss" },
        { shortcut: "q", text: "Close blade" }
      ]
    },
    {
      title: "Trash",
      items: [
        { shortcut: "j/k  g/G", text: "Move" },
        { shortcut: "Enter", text: "Restore" },
        { shortcut: "Delete", text: "Delete forever" },
        { shortcut: "Shift+E", text: "Empty trash" },
        { shortcut: "r", text: "Refresh" },
        { shortcut: "Esc", text: "Back" }
      ]
    },
    {
      title: "Search syntax",
      items: [
        { shortcut: "rdme", text: "Fuzzy (fzf)" },
        { shortcut: "'word", text: "Substring" },
        { shortcut: "^start  end$", text: "Anchors" },
        { shortcut: "\"phrase\"", text: "Exact match" },
        { shortcut: "-word  !word", text: "Exclude" },
        { shortcut: "type:folder", text: "Kind" },
        { shortcut: "format:png", text: "Extension" },
        { shortcut: "in:src", text: "Folder" },
        { shortcut: "content:\"text\"", text: "File contents" },
        { shortcut: "scope:everywhere", text: "Whole disk (plocate)" }
      ]
    }
  ]

  function takeFocus(part) {
    var mode = String(part || "")
    if (mode === "search") tree.focusSearch()
    else if (mode === "location") tree.focusLocation()
    else if (mode === "quicknav") quickNav.focusInput()
    else tree.focusTree()
  }

  function rememberRoot() {
    if (!module.context.state || !module.controller.stateReady) return
    var root = String(module.controller.rootPath || "")
    if (root !== "" && module.context.state.get("root", "") !== root) module.context.state.set("root", root)
  }

  function restoreRoot() {
    if (!module.context.state) return
    var root = String(module.context.state.get("root", "") || "")
    if (root === "" || root === String(module.controller.rootPath || "")) return
    module.controller.navigateToLocation(root, module.context.screen, "browse")
  }

  Component.onCompleted: restoreRoot()

  Connections {
    target: module.controller
    function onRootPathChanged() { module.rememberRoot() }
    function onStateReadyChanged() { if (module.controller.stateReady) module.restoreRoot() }
  }

  TreePane {
    id: tree
    anchors.fill: parent
    anchors.bottomMargin: pickerBar.visible ? pickerBar.height : 0
    controller: module.controller
    hostWindow: module.context.hostWindow
    context: module.context
    focusEnabled: module.context.bladeOpen
  }

  PickerBar {
    id: pickerBar
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.bottom: parent.bottom
    controller: module.controller
    hostWindow: module.context.hostWindow
    visible: module.controller.pickerActive && module.context.bladeOpen
    z: 45
  }

  QuickNavOverlay {
    id: quickNav
    anchors.fill: parent
    controller: module.controller
    context: module.context
    z: 50
  }

  Component {
    id: filesSettings

    FilesSettings {
      controller: module.controller
      pane: tree
    }
  }
}
