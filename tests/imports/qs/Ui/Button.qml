import QtQuick

Rectangle {
  id: root
  property string text: ""
  property string iconText: ""
  property string tooltipText: ""
  property bool selected: false
  property bool active: false
  property bool hasCursor: false
  property bool focusable: false
  property bool bordered: false
  property color foreground: "#ffffff"
  property color accent: "#61afef"
  property string fontFamily: ""
  property real fontSize: 12
  property real iconSize: 12
  property real horizontalPadding: 10
  property real verticalPadding: 6
  property bool leftAlign: false

  signal clicked()
  signal rightClicked()
  signal hovered(bool isHovered)

  color: "transparent"
  activeFocusOnTab: focusable
  implicitWidth: label.implicitWidth + horizontalPadding * 2
  implicitHeight: label.implicitHeight + verticalPadding * 2
  Keys.onReturnPressed: if (root.focusable) root.clicked()
  Keys.onEnterPressed: if (root.focusable) root.clicked()
  Keys.onSpacePressed: if (root.focusable) root.clicked()

  Text {
    id: label
    anchors.centerIn: parent
    text: root.text
    color: root.foreground
    font.family: root.fontFamily
    font.pixelSize: root.fontSize
  }
  MouseArea {
    anchors.fill: parent
    hoverEnabled: true
    acceptedButtons: Qt.LeftButton | Qt.RightButton
    onEntered: root.hovered(true)
    onExited: root.hovered(false)
    onClicked: function(mouse) { if (mouse.button === Qt.RightButton) root.rightClicked(); else root.clicked() }
  }
}
