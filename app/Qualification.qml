import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons

Item {
  id: probe
  required property var service
  property bool barVisible: false
  property string goblinsDirectory: Quickshell.env("FILEBLADE_GOBLINS_FIXTURE")
  property var goblinsManifest: null
  property string fixtureError: ""

  function objects() {
    var result = [], queue = [service, barWindow], seen = []
    while (queue.length && seen.length < 20000) {
      var item = queue.shift()
      if (!item || seen.indexOf(item) >= 0) continue
      seen.push(item)
      result.push(item)
      for (var key of ["children", "data", "instances"]) {
        var entries = item[key]
        if (entries && entries.length !== undefined)
          for (var i = 0; i < entries.length; i++) queue.push(entries[i])
      }
      for (var child of ["item", "contentItem"]) if (item[child]) queue.push(item[child])
    }
    return result
  }

  function geometry(item) {
    var point = item.mapToItem(null, 0, 0)
    if (item.hostWindow) {
      point.x += Number(item.hostWindow.surfaceOriginX) || 0
      point.y += Number(item.hostWindow.surfaceOriginY) || 0
    }
    return { x: point.x, y: point.y, width: item.width, height: item.height }
  }

  function snapshot() {
    var slots = [], popouts = [], stacks = []
    for (var item of objects()) {
      if (item.slotIndex !== undefined && item.moduleItem !== undefined && item.loadFailed !== undefined && item.bladeOpen && item.hostActive) {
        slots.push({ edge: item.edge, index: item.slotIndex, module: item.moduleId,
          loaded: !!item.moduleItem, failed: item.loadFailed, title: item.title,
          providerError: item.providerError, geometry: geometry(item), collapsed: service.bladeHost.slotCollapsed(item.edge, item.slotIndex) })
      }
      if (item.moduleContext !== undefined && item.popoutState !== undefined)
        popouts.push({ module: item.moduleId, opened: item.opened, loaded: !!item.moduleItem, notice: item.notice, title: item.title })
      if (item.dropLineY && item.slotItem && item.bladeOpen)
        stacks.push({ edge: item.edge, geometry: geometry(item), dropVisible: item.dropVisible, dropLineY: item.dropLineY(), dropTabSlot: item.dropTabSlot, dropTabBand: item.dropTabBand })
    }
    return { slots: slots, popouts: popouts, stacks: stacks, barLoaded: !!widget.item,
      barOpened: widget.item ? widget.item.opened : false, fixtureError: fixtureError,
      providerErrors: service.bladeHost.providerErrors,
      drag: { active: service.bladeHost.dragActive, edge: service.bladeHost.dropEdge,
        index: service.bladeHost.dropIndex, tabSlot: service.bladeHost.dropTabSlot,
        tabBand: service.bladeHost.dropTabBand, tabIndex: service.bladeHost.dropTabIndex,
        noop: service.bladeHost.dropNoop } }
  }

  function serviceFor(id) { return id === "data-goblin.fileblade" ? service : service.services[id] || null }

  FileView {
    path: probe.goblinsDirectory ? probe.goblinsDirectory + "/manifest.json" : ""
    printErrors: false
    onLoaded: {
      try {
        probe.goblinsManifest = JSON.parse(text())
      } catch (error) { probe.fixtureError = String(error) }
    }
  }

  PanelWindow {
    id: barWindow
    visible: probe.barVisible
    anchors { top: true; left: true }
    margins.left: 900
    implicitWidth: 40
    implicitHeight: Style.bar.sizeHorizontal
    color: Color.background
    exclusionMode: ExclusionMode.Ignore
    WlrLayershell.namespace: "fileblade-qualification-bar"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    QtObject {
      id: facade
      property var shell: probe
      property bool vertical: false
      property string position: "top"
      property int barSize: Style.bar.sizeHorizontal
      property string fontFamily: Style.font.family
      property color barForeground: Color.foreground
      property color urgent: Color.urgent
      property bool foregroundAnimationEnabled: true
      property var activePopout: null
      property var clickTargets: []
      function registerClickTarget(item) { clickTargets = clickTargets.concat([item]) }
      function unregisterClickTarget(item) { clickTargets = clickTargets.filter(function(entry) { return entry !== item }) }
      function hideTooltip(item) {}
      function showTooltip(item, text) {}
      function requestPopout(item) { activePopout = item }
      function releasePopout(item) { if (activePopout === item) activePopout = null }
      function moduleWidgets(name) { return widget.item ? [widget.item] : [] }
    }

    Loader {
      id: widget
      anchors.centerIn: parent
      onStatusChanged: if (status === Loader.Error) probe.fixtureError = "Goblins BarWidget import failed"
    }
  }

  IpcHandler {
    target: "fileblade.qualification"
    function status(): string { return JSON.stringify(probe.snapshot()) }
    function goblins(enabled: string): string {
      if (enabled === "true") {
        probe.service.extensionCatalog.providers = [{ id: probe.goblinsManifest.id, dir: probe.goblinsDirectory, enabled: true, manifest: probe.goblinsManifest }]
        probe.service.bladeHost.registry.rebuild()
        probe.barVisible = true
        widget.setSource(Util.fileUrl(probe.goblinsDirectory + "/GoblinBarWidget.qml"), {
          bar: facade, settings: { showNavbarIcon: true, width: 460, height: 600 }
        })
      } else {
        if (widget.item) widget.item.close()
        widget.source = ""
        probe.barVisible = false
      }
      return "ok"
    }
  }
}
