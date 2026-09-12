import QtQuick
import QtQuick.Effects
import Quickshell
import qs.Commons
import "../lib/FileIcons.js" as FileIcons

Item {
  id: root

  property string iconName: ""
  property string trustedIconSource: ""
  property bool monochrome: false
  property string monochromeMask: "alpha"
  property color iconColor: fallbackColor
  property string fallbackGlyph: ""
  property color fallbackColor: Color.accent
  property real fallbackSize: Style.font.body
  property real iconSize: Style.space(18)
  property string desktopId: ""
  property var applicationDescriptor: null
  property var applicationOverride: null
  readonly property var desktopEntry: desktopId === "" ? null : DesktopEntries.applications.values.find(function(entry) {
    return entry.id === root.desktopId.replace(/\.desktop$/, "")
  }) || null
  readonly property var applicationIcon: desktopId === "" && applicationDescriptor === null ? null
    : FileIcons.resolveApplication(applicationDescriptor || { icon: iconName, glyph: fallbackGlyph },
        applicationOverride, desktopEntry, Quickshell.iconPath)
  readonly property string resolvedSource: {
    if (applicationIcon !== null) return applicationIcon.icon_source
    if (trustedIconSource !== "") return trustedIconSource
    var safeName = FileIcons.safeThemeIconName(iconName)
    return safeName ? Quickshell.iconPath(safeName, true) : ""
  }

  width: iconSize
  height: iconSize

  Text {
    visible: root.fallbackGlyph !== "" && icon.status !== Image.Ready
    anchors.centerIn: parent
    textFormat: Text.PlainText
    text: root.fallbackGlyph
    color: root.fallbackColor
    font.family: Style.font.family
    font.pixelSize: root.fallbackSize
  }

  Image {
    id: icon
    visible: !root.monochrome && root.resolvedSource !== "" && icon.status === Image.Ready
    anchors.fill: parent
    fillMode: Image.PreserveAspectFit
    sourceSize.width: width * Screen.devicePixelRatio
    sourceSize.height: height * Screen.devicePixelRatio
    source: root.resolvedSource
    asynchronous: true
  }

  readonly property bool maskedByLuminance: monochromeMask === "luminance" || monochromeMask === "dark"

  MultiEffect {
    anchors.fill: icon
    source: icon
    visible: root.monochrome && !root.maskedByLuminance
      && root.resolvedSource !== "" && icon.status === Image.Ready
    colorization: 1
    colorizationColor: root.iconColor
  }

  ShaderEffect {
    anchors.fill: icon
    visible: root.monochrome && root.maskedByLuminance
      && root.resolvedSource !== "" && icon.status === Image.Ready
    property var source: icon
    property color glyphColor: root.iconColor
    fragmentShader: Qt.resolvedUrl(root.monochromeMask === "dark"
      ? "shaders/DarkGlyph.frag.qsb"
      : "shaders/LuminanceGlyph.frag.qsb")
  }
}
