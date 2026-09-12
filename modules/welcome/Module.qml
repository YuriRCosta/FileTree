import QtQuick
import QtQuick.Controls as Controls
import qs.Commons
import "../../ui" as PluginUi
import "WelcomePlan.js" as WelcomePlan

FocusScope {
  id: module
  property var context: null
  readonly property string title: "Welcome"
  readonly property var files: context ? context.service("files") : null
  readonly property var host: files ? files.bladeHost : null
  readonly property var shortcuts: [
    { title: "Welcome", items: [{ shortcut: "Enter", text: "Open Files" }, { shortcut: "Esc", text: "Close Welcome" }] }
  ]
  property var catalog: WelcomePlan.LOCAL_CATALOG
  property int helpIndex: 0
  property string error: ""

  function takeFocus(part) { module.forceActiveFocus() }
  function info(id) { return host && host.registry ? host.registry.module(id) : null }
  function acceptCatalog(text) {
    var next = WelcomePlan.catalog(text, context ? context.contractVersion : 0)
    if (!next) return false
    catalog = next
    return true
  }
  function openModule(id) {
    if (!info(id)) { error = "This blade is unavailable."; return false }
    var found = host.findModule(id)
    if (!found) {
      var welcome = host.findModule("welcome")
      if (!host.addSlot(welcome ? welcome.edge : "right", id, -1)) { error = "The blade could not be added."; return false }
      found = host.findModule(id)
    }
    if (!found) return false
    host.setSlotTab(found.edge, found.index, found.tab)
    host.setOpen(found.edge, true)
    host.focusModule(id, host.preferredScreen(found.edge), "")
    error = ""
    return true
  }
  function dismiss() {
    if (!files || !files.welcome.dismiss()) error = "Welcome could not be closed. Try again when the layout is writable."
  }
  Keys.onPressed: function(event) {
    if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) { openModule("files"); event.accepted = true }
    else if (event.key === Qt.Key_Escape) { dismiss(); event.accepted = true }
  }

  component Action: Controls.Button {
    id: action
    property string detail: ""
    property string glyph: ""
    property string iconUrl: ""
    width: parent ? parent.width : 0
    padding: Style.space(8)
    implicitHeight: body.implicitHeight + padding * 2
    hoverEnabled: true
    Keys.onReturnPressed: clicked()
    Keys.onEnterPressed: clicked()
    onActiveFocusChanged: {
      if (activeFocus) scroll.contentY = Math.max(0, Math.min(mapToItem(content, 0, 0).y, scroll.contentHeight - scroll.height))
    }
    background: Rectangle {
      radius: 0
      color: action.hovered || action.activeFocus ? Util.alpha(Color.accent, 0.15) : "transparent"
      border.width: action.activeFocus ? 1 : 0
      border.color: Color.accent
    }
    contentItem: Row {
      spacing: Style.space(8)
      PluginUi.ModuleIcon {
        width: Style.space(16)
        height: width
        glyph: action.glyph
        iconUrl: action.iconUrl
        visible: glyph !== "" || iconUrl !== ""
        color: Color.accent
      }
      Column {
        id: body
        width: parent.width - (action.glyph !== "" || action.iconUrl !== "" ? Style.space(24) : 0)
        spacing: Style.space(3)
        Text {
          width: parent.width
          textFormat: Text.PlainText
          text: action.text
          color: action.enabled ? Color.bar.text : Color.muted
          wrapMode: Text.WordWrap
          font.family: Style.font.family
          font.pixelSize: Style.font.body
        }
        Text {
          width: parent.width
          visible: text !== ""
          textFormat: Text.PlainText
          text: action.detail
          color: Color.muted
          wrapMode: Text.WordWrap
          font.family: Style.font.family
          font.pixelSize: Style.font.bodySmall
        }
      }
    }
  }

  Rectangle { anchors.fill: parent; color: Qt.lighter(Color.background, 1.035) }
  Flickable {
    id: scroll
    anchors.fill: parent
    anchors.margins: Style.space(12)
    contentWidth: width
    contentHeight: content.height
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    Controls.ScrollBar.vertical: Controls.ScrollBar {}
    Column {
      id: content
      width: scroll.width
      spacing: Style.space(8)
      Text {
        width: parent.width
        textFormat: Text.PlainText
        text: WelcomePlan.HEADING
        color: Color.bar.text
        font.family: Style.font.family
        font.pixelSize: Style.font.body
        font.weight: Font.DemiBold
      }
      Text {
        width: parent.width
        textFormat: Text.PlainText
        text: WelcomePlan.BODY
        color: Color.muted
        wrapMode: Text.WordWrap
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
      }
      Repeater {
        model: WelcomePlan.CORE
        delegate: Action {
          required property var modelData
          readonly property var entry: module.info(modelData.id)
          text: modelData.name
          detail: modelData.description
          glyph: entry ? entry.glyph : ""
          iconUrl: entry ? entry.iconUrl : ""
          enabled: !!entry
          onClicked: module.openModule(modelData.id)
        }
      }
      PluginUi.SettingsGroup { title: "Help · available offline" }
      Repeater {
        model: WelcomePlan.HELP
        delegate: Action {
          required property var modelData
          required property int index
          text: modelData.name
          detail: module.helpIndex === index ? modelData.text : ""
          onClicked: module.helpIndex = index
        }
      }
      Action { text: "Keyboard reference"; onClicked: Qt.openUrlExternally(Qt.resolvedUrl("../../docs/agent-written/keybindings.md")) }
      Action { text: "Extension authoring guide"; onClicked: Qt.openUrlExternally(Qt.resolvedUrl("../../EXTENSIONS.md")) }
      PluginUi.SettingsGroup { title: "Extensions" }
      Text {
        width: parent.width
        visible: module.catalog.entries.length === 0
        textFormat: Text.PlainText
        text: "Extensions can add more blades. No optional extensions are listed here yet. Installed blades remain available from +."
        color: Color.muted
        wrapMode: Text.WordWrap
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
      }
      Repeater {
        model: module.catalog.entries
        delegate: Action {
          required property var modelData
          text: modelData.name
          detail: modelData.id + " · " + modelData.lifecycle + "\n" + (modelData.compatible ? "Compatible" : "Requires a newer extension interface") + "\n" + modelData.source
          onClicked: Qt.openUrlExternally(modelData.source)
        }
      }
      Text {
        width: parent.width
        visible: text !== ""
        textFormat: Text.PlainText
        text: module.error
        color: Color.urgent
        wrapMode: Text.WordWrap
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
      }
      Action { text: WelcomePlan.DISMISS; detail: "Reopen Welcome with + in a blade."; onClicked: module.dismiss() }
    }
  }
}
