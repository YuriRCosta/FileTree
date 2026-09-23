import QtQuick
import "../lib/MonitorMode.js" as MonitorMode
import QtQuick.Controls
import qs.Commons
import qs.Ui
import "../ui" as PluginUi
import "../theme"

Item {
  id: root

  required property var host
  required property var surface

  readonly property string edge: surface.edge
  property string query: ""
  property int moduleShown: 0
  readonly property bool filtering: query.trim() !== ""
  property int pendingRetentionDays: 0

  function confirmTrashRetention(days) {
    pendingRetentionDays = days
    retentionConsent.open("Enable automatic Trash cleanup?\nPermanently delete items older than " + days + " days from your shared desktop Trash, including items trashed by other apps, and FileBlade artifact bins. Cleanup also runs while blades are closed. This cannot be undone.",
      [{ key: "cancel", label: "Cancel" }, { key: "enable", label: "Enable cleanup", danger: true }])
  }

  PluginUi.ActionDialog {
    id: retentionConsent
    anchors.fill: parent
    z: 1000
    onChosen: function(key) { if (key === "enable") root.host.service.setTrashRetentionDays(root.pendingRetentionDays, true) }
  }

  function close() {
    host.setSettingsOpen(false, edge)
    host.focusBlade(edge, surface.screen, -1, "")
  }

  function openShortcuts() {
    host.setSettingsOpen(false, edge)
    surface.shortcutsOpen = true
  }

  function confirmRevert() {
    revertDialog.open(
      "Are you sure?\nThis will revert to default settings and layouts. It doesn't affect key bindings.",
      [{ key: "revert", label: "Yes", danger: true }, { key: "cancel", label: "Cancel" }])
    revertDialog.selectedIndex = 1
  }

  function slotSettings() {
    var result = []
    for (var i = 0; i < surface.slots.length; i++) {
      var item = surface.slotItem(i)
      if (!item) continue
      var context = item.settingsContext || null
      var declared = !!context && !!context.settings && context.settings.schema.length > 0
      if (item.settingsComponent || declared)
        result.push({ title: String(item.title || ""), component: item.settingsComponent || null, context: context })
    }
    return result
  }

  function matches(haystack) {
    var needle = query.trim().toLowerCase()
    if (needle === "") return true
    var hay = String(haystack).toLowerCase()
    var terms = needle.split(/\s+/)
    for (var i = 0; i < terms.length; i++)
      if (hay.indexOf(terms[i]) < 0) return false
    return true
  }

  function rowHaystack(item, title) {
    var parts = [String(title || "")]
    if (item.label !== undefined) parts.push(String(item.label))
    else if (item.shortcut !== undefined && item.text !== undefined) parts.push(String(item.text))
    else return null
    if (item.detail !== undefined) parts.push(String(item.detail))
    if (item.group !== undefined) parts.push(String(item.group))
    var options = item.options
    if (options && options.length !== undefined)
      for (var i = 0; i < options.length; i++)
        parts.push(String(options[i] && options[i].label !== undefined ? options[i].label : options[i]))
    return parts.join(" ")
  }

  function recount() {
    var total = 0
    for (var i = 0; i < moduleRepeater.count; i++) {
      var section = moduleRepeater.itemAt(i)
      if (section) total += section.shown
    }
    moduleShown = total
  }

  onVisibleChanged: {
    if (!visible) return
    search.text = ""
    Qt.callLater(function() { search.forceActiveFocus() })
  }

  focus: visible
  Keys.onPressed: function(event) {
    if (event.key === Qt.Key_Escape) {
      root.close()
      event.accepted = true
    }
  }

  component SectionLabel: Text {
    textFormat: Text.PlainText
    width: parent ? parent.width : 0
    color: Color.muted
    elide: Text.ElideRight
    font.family: Style.font.family
    font.pixelSize: Typography.caption
    font.letterSpacing: 0.4
  }

  component Divider: Rectangle {
    width: parent ? parent.width : 0
    height: 1
    color: Util.alpha(Color.bar.text, 0.12)
  }

  component Link: Text {
    id: link
    signal clicked()
    textFormat: Text.PlainText
    color: linkPointer.containsMouse ? Color.bar.text : Color.muted
    font.family: Style.font.family
    font.pixelSize: Typography.caption

    MouseArea {
      id: linkPointer
      anchors.fill: parent
      anchors.margins: -Style.space(4)
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: link.clicked()
    }
  }

  Rectangle {
    anchors.fill: parent
    color: Util.alpha(Color.background, 0.48)

    MouseArea {
      anchors.fill: parent
      onClicked: root.close()
    }
  }

  Rectangle {
    id: card
    readonly property int pad: Style.space(10)
    anchors.top: parent.top
    anchors.topMargin: Style.space(35)
    anchors.right: root.edge === "left" ? parent.right : undefined
    anchors.rightMargin: Style.space(8)
    anchors.left: root.edge === "right" ? parent.left : undefined
    anchors.leftMargin: Style.space(8)
    width: Math.min(parent.width - Style.space(16), Style.space(520))
    height: Math.min(parent.height - Style.space(50), chrome.height + Math.ceil(settingsContent.implicitHeight) + pad * 2)
    radius: Style.cornerRadius
    color: Color.popups.background
    border.width: 1
    border.color: Color.popups.border
    clip: true

    MouseArea {
      anchors.fill: parent
      acceptedButtons: Qt.AllButtons
    }

    Column {
      id: chrome
      anchors.top: parent.top
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.margins: card.pad
      anchors.bottomMargin: 0
      spacing: Style.space(8)

      Item {
        width: parent.width
        height: Style.space(24)

        Text {
          textFormat: Text.PlainText
          anchors.left: parent.left
          anchors.verticalCenter: parent.verticalCenter
          text: "SETTINGS"
          color: Color.bar.text
          font.family: Style.font.family
          font.pixelSize: Typography.bodySmall
          font.weight: Font.DemiBold
          font.letterSpacing: 0.5
        }

        Text {
          textFormat: Text.PlainText
          anchors.right: parent.right
          anchors.verticalCenter: parent.verticalCenter
          text: "×"
          color: closePointer.containsMouse ? Color.bar.text : Color.muted
          font.family: Style.font.family
          font.pixelSize: Typography.title

          MouseArea {
            id: closePointer
            anchors.fill: parent
            anchors.margins: -Style.space(7)
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.close()
          }
        }
      }

      PluginUi.MenuSearchField {
        id: search
        width: parent.width
        height: Style.space(30)
        prompt: "Search settings"
        textColor: Color.bar.text
        onTextChanged: root.query = text
        onDismissed: {
          if (text !== "") text = ""
          else root.close()
        }
      }

      Item {
        width: parent.width
        height: Style.space(2)
      }
    }

    Flickable {
      id: flick
      anchors.top: chrome.bottom
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.bottom: parent.bottom
      anchors.leftMargin: card.pad
      anchors.rightMargin: card.pad
      anchors.bottomMargin: card.pad
      contentWidth: width
      contentHeight: Math.ceil(settingsContent.implicitHeight)
      interactive: contentHeight > height
      clip: true
      boundsBehavior: Flickable.StopAtBounds
      ScrollBar.vertical: PluginUi.AccentScrollBar { }

      Column {
        id: settingsContent
        width: parent.width
        spacing: Style.space(6)

        Column {
          id: bladesBlock
          readonly property var bladeData: root.host.bladeFor(root.host.side)
          readonly property bool headerShown: root.matches("blade open closed width " + bladeData.width + " px")
          readonly property bool sideShown: root.matches("blade side left right edge position")
          readonly property bool shown: headerShown || sideShown
          width: parent.width
          spacing: Style.space(5)
          visible: shown

          PluginUi.ToggleRow {
            width: parent.width
            visible: bladesBlock.headerShown
            heading: true
            label: "BLADE"
            detail: bladesBlock.bladeData.width + " px"
            checked: !!bladesBlock.bladeData.open
            onToggled: root.host.toggleOpen(root.host.side)
          }

          PluginUi.DropdownRow {
            width: parent.width
            visible: bladesBlock.sideShown
            glyph: "󰞘"
            label: "Side"
            prompt: "Pick a side…"
            options: [{ key: "left", label: "Left" }, { key: "right", label: "Right" }]
            value: root.host.side
            onChosen: function(key) { root.host.setSide(key) }
          }
        }

        Column {
          id: general
          readonly property bool monitorsShown: root.matches("general monitors screens display active primary all")
          readonly property bool fontScaleShown: root.matches("general font size text scale typography percent")
          readonly property bool shown: monitorsShown || fontScaleShown
          width: parent.width
          spacing: Style.space(5)
          visible: shown

          Divider { visible: bladesBlock.shown }

          SectionLabel { text: "GENERAL" }

          PluginUi.DropdownRow {
            width: parent.width
            visible: general.monitorsShown
            glyph: "󰍹"
            label: "Monitors"
            prompt: "Find monitor…"
            options: MonitorMode.choices(root.host.screenNames)
            value: MonitorMode.choiceKey(root.host.monitorMode, root.host.monitorLock)
            onChosen: function(key) { var choice = MonitorMode.parseChoice(key); root.host.setMonitorMode(choice.mode, choice.lock) }
          }

          PluginUi.NumberRow {
            width: parent.width
            visible: general.fontScaleShown
            row: ({ key: "fontScale", type: "integer", label: "Font size %", glyph: "󰛖",
                    min: Typography.minimumPercent, max: Typography.maximumPercent,
                    step: Typography.percentStep, defaultValue: 100, value: Typography.percent })
            onCommitted: function(value) { root.host.setFontScale(Typography.scaleFromPercent(value)) }
          }
        }

        Repeater {
          id: moduleRepeater
          model: root.visible ? root.slotSettings() : []
          onItemAdded: root.recount()
          onItemRemoved: root.recount()

          delegate: BladeModuleSection {
            sheet: root
            dividerShown: bladesBlock.shown || general.shown || index > 0
          }
        }

        Column {
          id: footer
          readonly property bool layoutShown: root.matches("layout file config json " + root.host.layoutPath)
          readonly property bool shortcutsShown: root.matches("shortcuts keys keyboard help ?")
          readonly property bool revertShown: root.matches("revert reset default settings layouts")
          readonly property bool shown: layoutShown || shortcutsShown || revertShown
          width: parent.width
          spacing: Style.space(5)
          visible: shown

          Divider { visible: bladesBlock.shown || general.shown || root.moduleShown > 0 }

          PluginUi.HintLine {
            width: parent.width
            visible: footer.layoutShown
            glyph: "󰈔"
            text: root.host.layoutPath
          }

          PluginUi.HintLine {
            id: shortcutsLine
            width: parent.width
            visible: footer.shortcutsShown
            shortcut: "?"
            text: "Shortcuts"
            foreground: shortcutsPointer.containsMouse ? Color.accent : Color.bar.text

            MouseArea {
              id: shortcutsPointer
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.openShortcuts()
            }
          }

          Link {
            visible: footer.revertShown
            text: "Revert to default settings"
            onClicked: root.confirmRevert()
          }
        }

        Text {
          textFormat: Text.PlainText
          width: parent.width
          visible: root.filtering && !bladesBlock.shown && !general.shown && root.moduleShown === 0 && !footer.shown
          text: "No setting matches \"" + root.query.trim() + "\""
          color: Color.muted
          elide: Text.ElideRight
          font.family: Style.font.family
          font.pixelSize: Typography.caption
        }
      }
    }
    PluginUi.ActionDialog {
      id: revertDialog
      anchors.fill: parent
      z: 40
      onChosen: function(key) { if (key === "revert") root.host.revertDefaults() }
    }
  }
}
