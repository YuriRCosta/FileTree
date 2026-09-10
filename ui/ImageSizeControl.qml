import QtQuick
import qs.Commons
import "../lib/ImageGallery.js" as ImageGallery

Item {
  id: control

  property int step: ImageGallery.DEFAULT_STEP
  property string shortcutSmaller: "-"
  property string shortcutLarger: "+"
  readonly property int steps: ImageGallery.stepCount()
  readonly property int clamped: ImageGallery.clampStep(step)
  signal stepRequested(int step)

  implicitWidth: row.implicitWidth
  implicitHeight: Style.space(24)

  function request(next) {
    var wanted = ImageGallery.clampStep(next)
    if (wanted !== clamped) stepRequested(wanted)
  }

  Row {
    id: row
    anchors.verticalCenter: parent.verticalCenter
    spacing: Style.space(2)

    Rectangle {
      id: smaller
      width: Style.space(20)
      height: Style.space(24)
      radius: Math.min(Style.cornerRadius, Style.space(4))
      color: smallerPointer.containsMouse ? Color.menu.selectedBackground : "transparent"
      opacity: control.clamped > 0 ? 1 : 0.32

      Text {
        textFormat: Text.PlainText
        anchors.centerIn: parent
        text: "−"
        color: Color.bar.text
        font.family: Style.font.family
        font.pixelSize: Style.font.body
      }

      MouseArea {
        id: smallerPointer
        anchors.fill: parent
        hoverEnabled: true
        enabled: control.clamped > 0
        cursorShape: Qt.PointingHandCursor
        onClicked: control.request(control.clamped - 1)
      }

      HintTip {
        visible: smallerPointer.containsMouse
        title: "Smaller previews"
        actions: [{ button: "left", text: "Step down" }, { shortcut: control.shortcutSmaller }]
      }
    }

    Row {
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(3)

      Repeater {
        model: control.steps

        delegate: Rectangle {
          id: dot
          required property int index
          width: Style.space(4 + index)
          height: width
          radius: width / 2
          anchors.verticalCenter: parent ? parent.verticalCenter : undefined
          color: index <= control.clamped ? Color.accent : Util.alpha(Color.bar.text, 0.22)

          MouseArea {
            anchors.fill: parent
            anchors.margins: -Style.space(2)
            cursorShape: Qt.PointingHandCursor
            onClicked: control.request(dot.index)
          }
        }
      }
    }

    Rectangle {
      id: larger
      width: Style.space(20)
      height: Style.space(24)
      radius: Math.min(Style.cornerRadius, Style.space(4))
      color: largerPointer.containsMouse ? Color.menu.selectedBackground : "transparent"
      opacity: control.clamped < control.steps - 1 ? 1 : 0.32

      Text {
        textFormat: Text.PlainText
        anchors.centerIn: parent
        text: "+"
        color: Color.bar.text
        font.family: Style.font.family
        font.pixelSize: Style.font.body
      }

      MouseArea {
        id: largerPointer
        anchors.fill: parent
        hoverEnabled: true
        enabled: control.clamped < control.steps - 1
        cursorShape: Qt.PointingHandCursor
        onClicked: control.request(control.clamped + 1)
      }

      HintTip {
        visible: largerPointer.containsMouse
        title: "Larger previews"
        actions: [{ button: "left", text: "Step up" }, { shortcut: control.shortcutLarger }]
      }
    }
  }
}
