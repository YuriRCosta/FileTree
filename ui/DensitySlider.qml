import QtQuick
import QtQuick.Controls
import qs.Commons

FocusScope {
  id: control

  property int step: 2
  property string label: "View size"
  property string shortcutSmaller: "-"
  property string shortcutLarger: "+"
  property var labels: ["XS", "S", "M", "L", "XL"]
  readonly property int steps: 5
  readonly property int clamped: Math.max(0, Math.min(steps - 1, step))
  readonly property string valueLabel: String(labels[clamped] || clamped + 1)
  readonly property bool pressed: range.pressed
  signal stepRequested(int step)
  signal keyPressed(var event)

  activeFocusOnTab: true

  implicitWidth: Style.space(156)
  implicitHeight: Style.space(28)

  function request(next) {
    var value = Math.floor(Number(next))
    var wanted = Math.max(0, Math.min(steps - 1, isFinite(value) ? value : 2))
    if (wanted !== clamped) stepRequested(wanted)
  }

  function takeFocus() { range.focus = false; control.forceActiveFocus() }

  Keys.onPressed: function(event) {
    if (event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)) { keyPressed(event); return }
    if ([Qt.Key_Left, Qt.Key_Down, Qt.Key_Minus, Qt.Key_Underscore].indexOf(event.key) >= 0) request(clamped - 1)
    else if ([Qt.Key_Right, Qt.Key_Up, Qt.Key_Plus, Qt.Key_Equal].indexOf(event.key) >= 0) request(clamped + 1)
    else if (event.key === Qt.Key_Home) request(0)
    else if (event.key === Qt.Key_End) request(steps - 1)
    else { keyPressed(event); return }
    event.accepted = true
  }

  Text {
    id: smaller
    width: Style.space(24)
    height: parent.height
    textFormat: Text.PlainText
    text: "−"
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
    color: control.clamped > 0 ? Color.bar.text : Color.muted
    font.family: Style.font.family
    font.pixelSize: Style.font.body
    Accessible.role: Accessible.Button
    Accessible.name: "Decrease " + control.label
    Accessible.onPressAction: { control.takeFocus(); control.request(control.clamped - 1) }
    MouseArea {
      anchors.fill: parent
      cursorShape: Qt.PointingHandCursor
      onClicked: { control.takeFocus(); control.request(control.clamped - 1) }
    }
  }

  Slider {
    id: range
    objectName: "densityRange"
    anchors.left: smaller.right
    anchors.right: larger.left
    height: parent.height
    from: 0
    to: control.steps - 1
    stepSize: 1
    value: control.clamped
    snapMode: Slider.SnapAlways
    live: true
    padding: 0
    focusPolicy: Qt.NoFocus
    onPressedChanged: if (pressed) control.takeFocus()
    onActiveFocusChanged: if (activeFocus) control.takeFocus()
    Accessible.focusable: true
    Accessible.focused: control.activeFocus
    Accessible.onIncreaseAction: control.request(control.clamped + 1)
    Accessible.onDecreaseAction: control.request(control.clamped - 1)
    Accessible.name: control.label
    Accessible.description: control.valueLabel + ". Five positions. Arrow keys, Home and End change size. Smaller: " + control.shortcutSmaller + "; larger: " + control.shortcutLarger
    onMoved: control.request(value)

    background: Rectangle {
      x: range.handle.width / 2
      y: (range.height - height) / 2
      width: Math.max(0, range.width - range.handle.width)
      height: Math.max(1, Style.space(2))
      color: Color.muted
    }

    handle: Item {
      x: range.visualPosition * Math.max(0, range.width - width)
      y: (range.height - height) / 2
      width: Style.space(28)
      height: Style.space(28)

      Rectangle {
        anchors.fill: parent
        anchors.margins: Style.space(2)
        color: "transparent"
        border.width: control.activeFocus ? 1 : 0
        border.color: Color.accent
      }

      Rectangle {
        anchors.centerIn: parent
        width: Style.space(16)
        height: width
        color: Color.bar.background
      }

      Text {
        anchors.centerIn: parent
        textFormat: Text.PlainText
        text: "\ue900"
        color: Color.accent
        font.family: "omarchy"
        font.pixelSize: Style.space(16)
      }
    }
  }

  Text {
    id: larger
    anchors.right: valueText.left
    width: Style.space(24)
    height: parent.height
    textFormat: Text.PlainText
    text: "+"
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
    color: control.clamped < control.steps - 1 ? Color.bar.text : Color.muted
    font.family: Style.font.family
    font.pixelSize: Style.font.body
    Accessible.role: Accessible.Button
    Accessible.name: "Increase " + control.label
    Accessible.onPressAction: { control.takeFocus(); control.request(control.clamped + 1) }
    MouseArea {
      anchors.fill: parent
      cursorShape: Qt.PointingHandCursor
      onClicked: { control.takeFocus(); control.request(control.clamped + 1) }
    }
  }

  Text {
    id: valueText
    anchors.right: parent.right
    width: Style.space(24)
    height: parent.height
    textFormat: Text.PlainText
    text: control.valueLabel
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
    color: Color.muted
    font.family: Style.font.family
    font.pixelSize: Style.font.caption
    Accessible.ignored: true
  }
}
