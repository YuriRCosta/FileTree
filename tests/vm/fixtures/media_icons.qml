import QtQuick
import Quickshell
import Quickshell.Io
import "../../../lib/FileIcons.js" as FileIcons
import "../../../ui" as Ui
import "../../../panes" as Panes

ShellRoot {
  id: probe
  property string pluginRoot: ""
  property var icon: null
  property var mark: null
  property var menu: null
  property var failure: null
  property int fallbackStage: 0
  property int attempts: 0
  property var checks: []

  FloatingWindow { visible: true; implicitWidth: 400; implicitHeight: 200; Item { id: suite; anchors.fill: parent } }
  Component { id: iconFactory; Ui.SafeApplicationIcon {} }
  Component { id: menuFactory; Panes.MenuButton { menu: ({}) } }
  FileView {
    id: menuSource
    path: probe.pluginRoot + "/panes/FileActionsMenu.qml"
    blockLoading: true
  }

  function check(condition, detail) { if (!condition) throw new Error(detail) }

  Component.onCompleted: {
    icon = iconFactory.createObject(suite, { desktopId: "nvim.desktop", fallbackGlyph: "N" })
    mark = iconFactory.createObject(suite, { applicationDescriptor: { icon: "herdr", glyph: "H" }, x: 30 })
    menu = menuFactory.createObject(suite, { appIcon: "nvim", width: 200, y: 50 })
    failure = iconFactory.createObject(suite, {
      applicationDescriptor: { icon: "herdr", icon_source: "file:///brindle-missing-icon.svg", glyph: "H" },
      iconSize: 512, fallbackGlyph: "legacy"
    })
  }

  Timer {
    interval: 100
    repeat: true
    running: true
    onTriggered: {
      try {
        probe.check(++probe.attempts < 50, "Catalogue or fallback did not settle")
        var failedImage = probe.failure.children.find(function(child) { return child.sourceSize !== undefined })
        var fallbackText = probe.failure.children.find(function(child) { return child.textFormat !== undefined })
        if (probe.fallbackStage === 0) {
          if (failedImage.status !== Image.Ready) return
          probe.check(probe.failure.rejectedSources.length === 1 && probe.failure.resolvedSource.endsWith("/herdr.svg"), "Failed file did not fall through to bundled mark")
          probe.check(failedImage.sourceSize.width === 128 && failedImage.sourceSize.height === 128, "Large renderer exceeds decode cap")
          probe.failure.applicationDescriptor = { icon_source: "file:///brindle-missing-again.svg", glyph: "Z" }
          probe.check(probe.failure.rejectedSources.length === 0, "Descriptor change retained rejected sources")
          probe.fallbackStage = 1
          return
        }
        if (probe.fallbackStage === 1) {
          if (probe.failure.resolvedSource !== "") return
          probe.check(probe.failure.rejectedSources.length === 1 && fallbackText.visible && fallbackText.text === "Z", "Exhausted sources lost descriptor glyph")
          probe.failure.applicationOverride = { glyph: "X" }
          probe.check(probe.failure.rejectedSources.length === 0 && fallbackText.text === "X", "Override reset or glyph lost")
          probe.failure.applicationOverride = null
          probe.failure.applicationDescriptor = { icon: "herdr", glyph: "H" }
          probe.fallbackStage = 2
          return
        }
        if (failedImage.status !== Image.Ready) return
        probe.check(probe.failure.rejectedSources.length === 0 && !fallbackText.visible, "Replacement descriptor did not recover")
        var launcher = DesktopEntries.applications.values.find(function(entry) { return entry.id === "nvim" }) || null
        var image = probe.mark.children.find(function(child) { return child.sourceSize !== undefined })
        if (!launcher || !image || image.status !== Image.Ready) {
          return
        }
        probe.check(probe.icon.applicationIcon.icon === launcher.icon, "Launcher Icon identity differs")
        probe.check(probe.icon.resolvedSource === String(Quickshell.iconPath(launcher.icon, true)) && probe.icon.resolvedSource !== "", "Launcher theme lookup differs")
        probe.check(probe.mark.resolvedSource === Qt.resolvedUrl("../../../assets/marks/herdr.svg").toString(), "herdr mark lost")
        probe.check(image.sourceSize.width > 0 && image.sourceSize.width <= 128, "Renderer decode dimensions are unbounded")
        var legacy = iconFactory.createObject(suite, { iconName: "nvim" })
        probe.check(legacy.applicationIcon === null && legacy.resolvedSource === String(Quickshell.iconPath("nvim", true)), "Legacy iconName changed")
        legacy.trustedIconSource = probe.mark.resolvedSource
        probe.check(legacy.resolvedSource === probe.mark.resolvedSource, "Legacy trusted source changed")
        var custom = FileIcons.resolveApplication({ icon: "herdr", glyph: "H" }, { icon: "tmux" }, launcher, function() { return "" })
        probe.check(custom.icon_source.endsWith("/assets/marks/tmux.svg"), "Explicit mark override lost")
        probe.check(FileIcons.resolveApplication({ icon: "herdr" }, { glyph: "X" }, launcher, Quickshell.iconPath).glyph === "X", "Glyph override lost")
        probe.check(FileIcons.resolveApplication({ icon_source: "https://example.invalid/icon.png" }, null, null, function() { return "" }).icon_source === "", "Untrusted remote source admitted")
        probe.checks.push({ consumer: "SafeApplicationIcon", desktop_id: launcher.id, icon: launcher.icon, source: probe.icon.resolvedSource, herdr: probe.mark.resolvedSource, failed_asset_recovery: true, decode_cap: failedImage.sourceSize.width })

        var buttonIcon = probe.menu.children.find(function(child) { return child.desktopId !== undefined })
        probe.check(buttonIcon.desktopId === "" && buttonIcon.applicationIcon === null, "Legacy MenuButton changed")
        probe.menu.appDesktopId = "nvim.desktop"
        probe.check(buttonIcon.desktopId === "nvim.desktop" && buttonIcon.applicationIcon.icon === launcher.icon, "MenuButton lost desktop identity")
        probe.menu.appIcon = ""
        probe.check(buttonIcon.visible, "MenuButton hid the desktop icon")
        probe.checks.push({ consumer: "MenuButton", desktop_id: buttonIcon.desktopId })

        var source = menuSource.text()
        var start = source.indexOf("          Repeater {\n            id: applicationRepeater")
        var end = source.indexOf("\n\n          MenuButton {", start)
        probe.check(start >= 0 && end > start, "Open with row declaration not found")
        var block = source.slice(start, end).replace("delegate: MenuButton", "delegate: Panes.MenuButton")
        var item = Qt.createQmlObject('import QtQuick\nimport "file://' + probe.pluginRoot + '/panes" as Panes\nItem { id: root; property bool openWithMode: true; property string targetPath: "/photo.png"; property var calls: []; property alias rows: applicationRepeater; property var controller: ({ applicationModel: [{ desktop_id: "nvim.desktop", name: "Neovim", icon: "nvim", is_default: false }], openWithApplication: function(id, path) { root.calls.push([id, path]) } }); function matches(name) { return true }\n' + block + '\n}', suite)
        var row = item.rows.itemAt(0)
        probe.check(row && row.appDesktopId === "nvim.desktop", "Open with row lost desktop id")
        row.clicked()
        probe.check(JSON.stringify(item.calls) === '[["nvim.desktop","/photo.png"]]', "Open with dispatch changed")
        probe.checks.push({ consumer: "FileActionsMenu", calls: item.calls })
        console.log("MEDIA_ICONS_PASS " + JSON.stringify(probe.checks))
      } catch (error) {
        console.error("MEDIA_ICONS_FAIL " + error)
      }
      Qt.quit()
    }
  }
}
