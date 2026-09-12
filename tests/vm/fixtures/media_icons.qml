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
  property var wide: null
  property var legacyGlyph: null
  property var legacy: null
  property var openWithItem: null
  property var openWithRow: null
  property var launcher: null
  property bool evidenceMode: false
  property string wideAsset: ""
  property string evidenceTitle: ""
  property string evidenceDetail: ""
  property bool checksComplete: false
  property int evidenceStage: -1
  property int evidenceReadyStage: -1
  property int evidenceAttempts: 0
  property int failedSourcesObserved: 0
  property var recoveryFailures: []
  property bool recoveryPending: false
  property var evidenceResults: []
  property int fallbackStage: 0
  property int attempts: 0
  property var checks: []

  FloatingWindow {
    visible: true
    implicitWidth: probe.evidenceMode ? 640 : 400
    implicitHeight: probe.evidenceMode ? 320 : 200
    Rectangle { anchors.fill: parent; color: "#14151f"; visible: probe.evidenceMode }
    Item {
      id: suite
      anchors.fill: parent
      Text {
        id: evidenceTitleText
        visible: probe.evidenceMode
        x: 24
        y: 18
        width: parent.width - 48
        color: "#c0caf5"
        font.pixelSize: 24
        text: probe.evidenceTitle
      }
      Text {
        id: evidenceDetailText
        visible: probe.evidenceMode
        x: 24
        y: 54
        width: parent.width - 48
        color: "#a9b1d6"
        font.pixelSize: 15
        text: probe.evidenceDetail
      }
    }
  }
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
    legacyGlyph = iconFactory.createObject(suite, {
      trustedIconSource: "file:///brindle-legacy-missing.svg", fallbackGlyph: "L", fallbackSize: 48
    })
    if (probe.evidenceMode) {
      wide = iconFactory.createObject(suite, {
        applicationDescriptor: { icon_source: probe.wideAsset, glyph: "W" },
        iconSize: 512
      })
    }
  }

  function imageFor(item) {
    return item && item.children ? item.children.find(function(child) { return child.sourceSize !== undefined }) : null
  }

  function fallbackFor(item) {
    return item && item.children ? item.children.find(function(child) { return child.textFormat !== undefined }) : null
  }

  function evidenceId() {
    return ["E-39-01", "E-39-02", "E-39-03", "E-39-04", "E-39-05", "E-39-06"][probe.evidenceStage] || ""
  }

  function evidenceObservation() {
    var id = probe.evidenceId()
    if (id === "E-39-01") {
      var rowIcon = probe.menu && probe.menu.children.find(function(child) { return child.desktopId !== undefined })
      var rowImage = probe.imageFor(rowIcon)
      var rowApplication = rowIcon && rowIcon.applicationIcon
      return { id: id, desktop_id: rowIcon ? String(rowIcon.desktopId) : "", application_icon: rowApplication ? String(rowApplication.icon || "") : "", source: rowIcon ? String(rowIcon.resolvedSource || "") : "", image_ready: !!rowImage && rowImage.status === Image.Ready, dispatch: "qualified by the live application scenario" }
    }
    if (id === "E-39-02") {
      var markImage = probe.imageFor(probe.mark)
      var markFallback = probe.fallbackFor(probe.mark)
      return { id: id, source: String(probe.mark.resolvedSource || ""), image_ready: !!markImage && markImage.status === Image.Ready, fallback_visible: !!markFallback && markFallback.visible }
    }
    if (id === "E-39-03") {
      var failedFallback = probe.fallbackFor(probe.failure)
      return { id: id, source: String(probe.failure.resolvedSource || ""), rejected_sources: probe.failure.rejectedSources.length, fallback_visible: !!failedFallback && failedFallback.visible, fallback_text: failedFallback ? String(failedFallback.text || "") : "" }
    }
    if (id === "E-39-04") {
      var recoveredImage = probe.imageFor(probe.failure)
      var recoveredFallback = probe.fallbackFor(probe.failure)
      return { id: id, source: String(probe.failure.resolvedSource || ""), image_ready: !!recoveredImage && recoveredImage.status === Image.Ready, rejected_sources: probe.failure.rejectedSources.length, fallback_visible: !!recoveredFallback && recoveredFallback.visible, failed_sources_observed: probe.failedSourcesObserved, failed_sources: probe.recoveryFailures, failed_source_bound: 4 }
    }
    if (id === "E-39-05") {
      var wideImage = probe.imageFor(probe.wide)
      var width = wideImage ? Number(wideImage.sourceSize.width) : 0
      var height = wideImage ? Number(wideImage.sourceSize.height) : 0
      return { id: id, source: String(probe.wide.resolvedSource || ""), image_ready: !!wideImage && wideImage.status === Image.Ready, source_width: width, source_height: height, painted_width: wideImage ? wideImage.paintedWidth : 0, painted_height: wideImage ? wideImage.paintedHeight : 0, aspect_ratio: wideImage && wideImage.paintedHeight ? wideImage.paintedWidth / wideImage.paintedHeight : 0 }
    }
    if (id === "E-39-06") {
      var legacyImage = probe.imageFor(probe.legacy)
      var legacyText = probe.fallbackFor(probe.legacyGlyph)
      return { id: id, icon_name: String(probe.legacy.iconName || ""), desktop_id: String(probe.legacy.desktopId || ""), descriptor_path_active: probe.legacy.applicationIcon !== null, legacy_glyph_visible: legacyText.visible, legacy_glyph: legacyText.text, trusted_source: String(probe.legacy.trustedIconSource || ""), source: String(probe.legacy.resolvedSource || ""), image_ready: !!legacyImage && legacyImage.status === Image.Ready }
    }
    return null
  }

  function setEvidenceStage(stage) {
    probe.evidenceStage = stage
    probe.evidenceReadyStage = -1
    probe.evidenceAttempts = 0
    var views = [probe.icon, probe.mark, probe.menu, probe.failure, probe.wide, probe.legacy, probe.legacyGlyph, probe.openWithItem]
    for (var i = 0; i < views.length; i++) if (views[i]) views[i].visible = false
    if (stage === 0) {
      probe.evidenceTitle = "E-39-01  Open with row"
      probe.evidenceDetail = "Neovim • nvim.desktop • catalogue icon"
      probe.menu.visible = true
      probe.menu.text = "Neovim"
      probe.menu.x = 24
      probe.menu.y = 108
      probe.menu.width = 592
      probe.menu.height = 46
    } else if (stage === 1) {
      probe.evidenceTitle = "E-39-02  Bundled application mark"
      probe.evidenceDetail = "herdr.svg • no desktop theme icon • glyph H"
      probe.mark.x = 260
      probe.mark.y = 100
      probe.mark.iconSize = 128
      probe.mark.visible = true
    } else if (stage === 2) {
      probe.evidenceTitle = "E-39-03  Override glyph after failure"
      probe.evidenceDetail = "missing icon file • explicit glyph X"
      probe.failure.x = 260
      probe.failure.y = 100
      probe.failure.iconSize = 128
      probe.failure.visible = true
      probe.failure.applicationDescriptor = { icon_source: "file:///brindle-evidence-missing.svg", glyph: "H" }
      probe.failure.applicationOverride = { icon: "brindle-evidence-missing", glyph: "X" }
    } else if (stage === 3) {
      probe.evidenceTitle = "E-39-04  Failed source recovery"
      probe.evidenceDetail = "valid bundled mark • failed sources: 0 • four failed assets"
      probe.failure.x = 260
      probe.failure.y = 100
      probe.failure.iconSize = 128
      probe.failure.desktopId = "brindle-icon-failures.desktop"
      probe.failure.applicationDescriptor = { icon: "tmux", icon_source: "file://" + probe.pluginRoot + "/bad-source.svg", glyph: "H" }
      probe.failure.applicationOverride = { icon: "pane-horizontal", glyph: "X" }
      probe.recoveryPending = true
      probe.failure.visible = true
    } else if (stage === 4) {
      probe.evidenceTitle = "E-39-05  Bounded icon decode"
      probe.evidenceDetail = "requested 512 px • decoder cap 128 px per dimension"
      probe.wide.x = 190
      probe.wide.y = 84
      probe.wide.transformOrigin = Item.TopLeft
      probe.wide.scale = 0.5
      probe.wide.iconSize = 512
      probe.wide.visible = true
    } else if (stage === 5) {
      probe.evidenceTitle = "E-39-06  Legacy icon consumer"
      probe.evidenceDetail = "iconName nvim • trusted source • no catalogue identity"
      probe.legacy.x = 260
      probe.legacy.y = 100
      probe.legacy.iconSize = 128
      probe.legacy.visible = true
      probe.legacyGlyph.x = 420
      probe.legacyGlyph.y = 150
      probe.legacyGlyph.visible = true
    }
  }

  function evidenceReady(observed) {
    var id = probe.evidenceId()
    if (id === "E-39-01") return observed.image_ready && observed.desktop_id === "nvim.desktop" && observed.application_icon !== "" && observed.source !== ""
    if (id === "E-39-02") return observed.image_ready && observed.source.endsWith("/assets/marks/herdr.svg") && !observed.fallback_visible
    if (id === "E-39-03") return observed.source === "" && observed.rejected_sources === 1 && observed.fallback_visible && observed.fallback_text === "X"
    if (id === "E-39-04") return observed.image_ready && observed.source.endsWith("/assets/marks/herdr.svg") && observed.rejected_sources === 0 && observed.failed_sources.length === 4 && !observed.fallback_visible
    if (id === "E-39-05") return observed.image_ready && observed.source_width > 0 && observed.source_width <= 128 && observed.source_height > 0 && observed.source_height <= 128 && Math.abs(observed.aspect_ratio - 2) < 0.01
    if (id === "E-39-06") return observed.image_ready && observed.icon_name === "nvim" && observed.desktop_id === "" && !observed.descriptor_path_active && observed.legacy_glyph_visible && observed.legacy_glyph === "L" && observed.trusted_source.endsWith("/assets/marks/herdr.svg") && observed.source === observed.trusted_source
    return false
  }

  function announceEvidence(observed) {
    if (probe.evidenceReadyStage === probe.evidenceStage) return
    probe.evidenceReadyStage = probe.evidenceStage
    if (probe.evidenceId() === "E-39-04") probe.evidenceDetail = "valid bundled mark • failed sources: " + observed.rejected_sources + " • prior failed assets: " + observed.failed_sources.length
    if (probe.evidenceId() === "E-39-05") probe.evidenceDetail = "requested 512 px • observed " + observed.source_width + " × " + observed.source_height + " px"
    console.log("MEDIA_ICONS_EVIDENCE_READY " + probe.evidenceId() + " " + JSON.stringify(observed))
  }

  function captureEvidence(value: string): void {
    if (!probe.evidenceMode || probe.evidenceReadyStage !== probe.evidenceStage || value !== probe.evidenceId()) return
    var observed = probe.evidenceObservation()
    probe.evidenceResults = probe.evidenceResults.concat([{ id: value, observed: observed }])
    console.log("MEDIA_ICONS_EVIDENCE_PASS " + value + " " + JSON.stringify(observed))
    if (probe.evidenceStage === 5) {
      console.log("MEDIA_ICONS_EVIDENCE_DONE " + JSON.stringify(probe.evidenceResults))
      Qt.quit()
      return
    }
    probe.setEvidenceStage(probe.evidenceStage + 1)
  }

  function abortEvidence(): void { if (probe.evidenceMode) Qt.quit() }

  function tickEvidence() {
    if (probe.recoveryPending && probe.failure.resolvedSource === "" && probe.failure.rejectedSources.length === 4) {
      probe.recoveryFailures = probe.failure.rejectedSources.slice()
      probe.recoveryPending = false
      probe.failure.desktopId = ""
      probe.failure.applicationOverride = null
      probe.failure.applicationDescriptor = { icon: "herdr", glyph: "H" }
    }
    probe.check(++probe.evidenceAttempts < 120, "Evidence state did not settle: " + JSON.stringify(probe.evidenceObservation()))
    var observed = probe.evidenceObservation()
    if (probe.evidenceReady(observed)) probe.announceEvidence(observed)
  }

  IpcHandler {
    target: "media-icons-probe"
    function capture(value: string): void { probe.captureEvidence(value) }
    function abort(): void { probe.abortEvidence() }
  }

  Timer {
    interval: 100
    repeat: true
    running: true
    onTriggered: {
      if (probe.checksComplete) {
        try {
          probe.tickEvidence()
        } catch (error) {
          console.error("MEDIA_ICONS_FAIL " + error)
          Qt.quit()
        }
        return
      }
      var failed = false
      try {
        probe.check(++probe.attempts < 50, "Catalogue or fallback did not settle")
        var failedImage = probe.failure.children.find(function(child) { return child.sourceSize !== undefined })
        var fallbackText = probe.failure.children.find(function(child) { return child.textFormat !== undefined })
        if (probe.fallbackStage === 0) {
          if (failedImage.status !== Image.Ready) return
          probe.check(probe.failure.rejectedSources.length === 1 && probe.failure.resolvedSource.endsWith("/herdr.svg"), "Failed file did not fall through to bundled mark")
          probe.failedSourcesObserved = probe.failure.rejectedSources.length
          probe.check(probe.failedSourcesObserved <= 4, "Failed source bound exceeded")
          probe.check(failedImage.sourceSize.width === 128 && failedImage.sourceSize.height === 128, "Large renderer exceeds decode cap")
          probe.failure.applicationDescriptor = { icon_source: "file:///brindle-missing-again.svg", glyph: "H" }
          probe.check(probe.failure.rejectedSources.length === 0, "Descriptor change retained rejected sources")
          probe.failure.applicationOverride = { icon: "brindle-missing", glyph: "X" }
          probe.fallbackStage = 1
          return
        }
        if (probe.fallbackStage === 1) {
          if (probe.failure.resolvedSource !== "") return
          probe.check(probe.failure.rejectedSources.length === 1 && fallbackText.visible && fallbackText.text === "X", "Failed image did not render override glyph")
          probe.failure.applicationOverride = null
          probe.failure.applicationDescriptor = { icon: "herdr", glyph: "H" }
          probe.fallbackStage = 2
          return
        }
        if (failedImage.status !== Image.Ready) return
        probe.check(probe.failure.rejectedSources.length === 0 && !fallbackText.visible, "Replacement descriptor did not recover")
        var launcher = DesktopEntries.applications.values.find(function(entry) { return entry.id === "nvim" }) || null
        var image = probe.mark.children.find(function(child) { return child.sourceSize !== undefined })
        if (!launcher || !image || image.status !== Image.Ready) return
        if (probe.imageFor(probe.legacyGlyph).status !== Image.Error) return
        probe.check(probe.legacyGlyph.applicationIcon === null && probe.fallbackFor(probe.legacyGlyph).text === "L" && probe.fallbackFor(probe.legacyGlyph).visible, "Legacy fallback glyph changed")
        probe.launcher = launcher
        probe.check(probe.icon.applicationIcon.icon === launcher.icon, "Launcher Icon identity differs")
        probe.check(probe.icon.resolvedSource === String(Quickshell.iconPath(launcher.icon, true)) && probe.icon.resolvedSource !== "", "Launcher theme lookup differs")
        probe.check(probe.mark.resolvedSource === Qt.resolvedUrl("../../../assets/marks/herdr.svg").toString(), "herdr mark lost")
        probe.check(image.sourceSize.width > 0 && image.sourceSize.width <= 128, "Renderer decode dimensions are unbounded")
        probe.legacy = iconFactory.createObject(suite, { iconName: "nvim" })
        probe.check(probe.legacy.applicationIcon === null && probe.legacy.resolvedSource === String(Quickshell.iconPath("nvim", true)), "Legacy iconName changed")
        probe.legacy.trustedIconSource = probe.mark.resolvedSource
        probe.check(probe.legacy.resolvedSource === probe.mark.resolvedSource, "Legacy trusted source changed")
        var custom = FileIcons.resolveApplication({ icon: "herdr", glyph: "H" }, { icon: "tmux" }, launcher, function() { return "" })
        probe.check(custom.icon_source.endsWith("/assets/marks/tmux.svg"), "Explicit mark override lost")
        probe.check(FileIcons.resolveApplication({ icon: "herdr" }, { glyph: "X" }, launcher, Quickshell.iconPath).glyph === "X", "Glyph override lost")
        probe.check(FileIcons.resolveApplication({ icon_source: "https://example.invalid/icon.png" }, null, null, function() { return "" }).icon_source === "", "Untrusted remote source admitted")
        probe.checks.push({ consumer: "SafeApplicationIcon", desktop_id: launcher.id, icon: launcher.icon, source: probe.icon.resolvedSource, herdr: probe.mark.resolvedSource, failed_asset_recovery: true, decode_cap: failedImage.sourceSize.width, failed_source_bound: 4, failed_sources_observed: probe.failedSourcesObserved })

        var buttonIcon = probe.menu.children.find(function(child) { return child.desktopId !== undefined })
        probe.check(buttonIcon.desktopId === "" && buttonIcon.applicationIcon === null, "Legacy MenuButton changed")
        probe.menu.appDesktopId = "nvim.desktop"
        probe.check(buttonIcon.desktopId === "nvim.desktop" && buttonIcon.applicationIcon.icon === launcher.icon, "MenuButton lost desktop identity")
        probe.menu.appIcon = ""
        probe.check(buttonIcon.visible, "MenuButton hid the desktop icon")
        probe.checks.push({ consumer: "MenuButton", desktop_id: buttonIcon.desktopId })

        if (!probe.evidenceMode) {
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
          probe.openWithItem = item
          probe.openWithRow = row
          probe.checks.push({ consumer: "FileActionsMenu", calls: item.calls })
        } else {
          probe.checks.push({ consumer: "FileActionsMenu", qualification: "live scenario dispatch" })
        }
        console.log("MEDIA_ICONS_PASS " + JSON.stringify(probe.checks))
      } catch (error) {
        failed = true
        console.error("MEDIA_ICONS_FAIL " + error)
      }
      if (failed) {
        Qt.quit()
        return
      }
      if (probe.evidenceMode) {
        probe.checksComplete = true
        probe.setEvidenceStage(0)
        return
      }
      Qt.quit()
    }
  }
}
