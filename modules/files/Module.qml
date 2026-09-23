import QtQuick
import qs.Commons
import "../../panes"

FocusScope {
  id: module

  required property var context

  readonly property var controller: context.service("files")
  readonly property string title: "FileTree"
  readonly property Component settings: filesSettings
  readonly property bool propertiesShown: !module.context.state || module.context.state.get("propertiesShown", true) !== false
  readonly property real propertiesFraction: {
    var saved = Number(module.context.state ? module.context.state.get("propertiesFraction", 0.3) : 0.3)
    return isFinite(saved) ? Math.max(0.12, Math.min(0.8, saved)) : 0.3
  }
  property real dragFraction: -1
  readonly property real liveFraction: dragFraction > 0 ? dragFraction : propertiesFraction
  readonly property int splitHandleSize: Style.space(6)
  readonly property int propertiesHeight: Math.round(Math.max(0, height - splitHandleSize) * liveFraction)

  function setPropertiesShown(value) {
    return !!module.context.state && module.context.state.set("propertiesShown", value === true)
  }
  function keys(action) { return controller.keybindings.label(action) }
  readonly property var shortcuts: [
    {
      title: "FileTree",
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
    else if (mode === "properties" && properties.item) properties.item.forcePaneFocus()
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
    anchors.top: parent.top
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.bottom: module.propertiesShown ? splitHandle.top : parent.bottom
    anchors.bottomMargin: !module.propertiesShown && pickerBar.visible ? pickerBar.height : 0
    controller: module.controller
    hostWindow: module.context.hostWindow
    context: module.context
    focusEnabled: module.context.bladeOpen
    focusSibling: properties.item ? function() { properties.item.forcePaneFocus() } : null
  }

  Item {
    id: splitHandle
    visible: module.propertiesShown
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.bottom: properties.top
    height: module.splitHandleSize

    Rectangle {
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      height: 1
      color: splitPointer.containsMouse || splitPointer.pressed ? Color.accent : Util.alpha(Color.bar.text, 0.12)
    }

    MouseArea {
      id: splitPointer
      anchors.fill: parent
      hoverEnabled: true
      preventStealing: true
      cursorShape: Qt.SizeVerCursor
      onPositionChanged: function(mouse) {
        if (!pressed) return
        var y = splitHandle.mapToItem(module, 0, mouse.y).y
        var usable = Math.max(1, module.height - module.splitHandleSize)
        module.dragFraction = Math.max(0.12, Math.min(0.8, (module.height - y - module.splitHandleSize / 2) / usable))
      }
      onReleased: {
        if (module.dragFraction > 0 && module.context.state)
          module.context.state.set("propertiesFraction", Math.round(module.dragFraction * 1000) / 1000)
        module.dragFraction = -1
      }
      onCanceled: module.dragFraction = -1
    }
  }

  Loader {
    id: properties
    active: module.propertiesShown
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.bottom: parent.bottom
    anchors.bottomMargin: pickerBar.visible ? pickerBar.height : 0
    height: module.propertiesShown ? module.propertiesHeight : 0

    sourceComponent: PropertiesPane {
      controller: module.controller
      hostWindow: module.context.hostWindow
      context: module.context
      focusEnabled: module.context.bladeOpen
      focusSibling: function() { tree.focusTree() }
    }
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
      filesModule: module
    }
  }
}
