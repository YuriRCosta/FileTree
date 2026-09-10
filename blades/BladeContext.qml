import QtQuick
import "../lib/Format.js" as Format
import "../lib/SettingsForm.js" as SettingsForm
import "../lib/PathText.js" as PathText

QtObject {
  id: context

  property var host: null
  property var shell: null
  property var services: ({})
  property var hostWindow: null
  property var screen: null
  property string edge: "left"
  property int slotIndex: -1
  property string slotId: ""
  property string moduleId: ""
  property string moduleDir: ""
  property string providerId: ""
  property int tabIndex: 0
  property int tabCount: 1
  property int handleTab: -1
  property bool bladeOpen: false
  property bool bladeFocused: false
  property bool slotFocused: false
  property bool collapsed: false
  property var slotState: ({})
  property var definition: null
  property var modulePickerRequested: null
  property var popout: null
  property bool retired: false

  readonly property int contractVersion: host ? host.moduleContractVersion : 0
  readonly property int surfaceOriginX: hostWindow ? Number(hostWindow.surfaceOriginX) || 0 : 0
  readonly property int surfaceOriginY: hostWindow ? Number(hostWindow.surfaceOriginY) || 0 : 0
  readonly property string dragScope: hostWindow && hostWindow.dragScope ? String(hostWindow.dragScope) : "screen"
  readonly property bool docked: !popout && dragScope === "screen"
  readonly property bool inPopout: !!popout
  readonly property int cornerReserveRight: 0
  readonly property int cornerReserveLeft: 0
  readonly property bool dragging: handleDragging
  readonly property var providerService: service(providerId)
  readonly property string category: definition && definition.category ? String(definition.category) : ""
  readonly property string stateDir: host ? host.dirs.stateDir(moduleId) : ""
  readonly property string configDir: host ? host.dirs.configDir(moduleId) : ""
  readonly property bool dirsReady: host ? host.dirs.ready(moduleId) : false
  property bool handleDragging: false
  property real handleStartX: 0
  property real handleStartY: 0

  property QtObject state: QtObject {
    function get(key, fallback) {
      var value = context.slotState ? context.slotState[String(key)] : undefined
      return value === undefined ? fallback : value
    }

    function set(key, value) {
      if (context.retired) return false
      if (context.popout) return context.popout.setState(String(key), value)
      if (!context.host) return false
      var index = context.host.findSlotId(context.edge, context.slotId)
      if (index < 0 || context.host.slotModuleAt(context.edge, index, context.tabIndex) !== context.moduleId) return false
      return context.host.setTabStateValue(context.edge, index, context.tabIndex, String(key), value)
    }
  }

  property QtObject settings: QtObject {
    readonly property var spec: context.definition && context.definition.settings ? context.definition.settings : null
    readonly property var schema: spec && Array.isArray(spec.schema) ? spec.schema : []
    readonly property var defaults: spec && spec.defaults && typeof spec.defaults === "object" ? spec.defaults : ({})

    function row(key) {
      return SettingsForm.row(schema, key)
    }

    function has(key) {
      return row(key) !== null
    }

    function get(key) {
      var found = row(key)
      return found ? SettingsForm.effective(found, context.state.get(found.key, undefined)) : undefined
    }

    function set(key, value) {
      var found = row(key)
      if (!found) return false
      var coerced = SettingsForm.coerce(found, value)
      return coerced !== undefined && context.state.set(found.key, coerced)
    }
  }

  function ensureDirs(callback) {
    return host ? host.dirs.ensure(moduleId, callback) : false
  }

  function service(id) {
    var key = String(id || "")
    if (!key) return null
    if (services && services[key]) return services[key]
    return shell && typeof shell.serviceFor === "function" ? shell.serviceFor(key) : null
  }

  property QtObject ui: QtObject {
    readonly property string base: context.host ? context.host.pluginDir + "/ui/" : ""

    function url(name) {
      return base ? PathText.fileUrl(PathText.join(base, String(name) + ".qml")) : ""
    }
  }

  property QtObject paths: QtObject {
    function canonical(path) { return PathText.fileUrl(path) }
    function parent(path) { return PathText.parent(path) }
    function join(directory, name) { return PathText.join(directory, name) }
    function within(path, directory) { return PathText.within(path, directory) }
    function name(path) { return PathText.name(path) }
  }

  property QtObject metrics: QtObject {
    readonly property var textKeys: Format.TEXT_METRIC_KEYS

    function option(spec) { return Format.metricOption(spec) }
    function options(specs) { return Format.metricOptions(specs) }
    function kind(option) { return Format.metricKind(option) }
    function estimateTokens(byteLength) { return Format.estimateTokens(byteLength) }
  }

  function requestFocus(part) {
    if (retired) return
    if (popout) popout.focusModule(part || "")
    else if (host) host.focusBlade(edge, screen, slotIndex, part || "", false)
  }

  function focusNext() {
    if (host && !retired && !popout) host.focusRelativeSlot(edge, slotIndex, 1, screen)
  }

  function focusPrevious() {
    if (host && !retired && !popout) host.focusRelativeSlot(edge, slotIndex, -1, screen)
  }

  function openSettings() {
    if (host) host.setSettingsOpen(true, edge)
  }

  function toggleSettings() {
    if (host) host.toggleSettings(edge)
  }

  function closeBlade() {
    if (popout) popout.close()
    else if (host) host.setOpen(edge, false, true)
  }

  function setCollapsed(value) {
    if (!host || popout) return false
    var desired = !!value
    if (!host.setSlotCollapsed(edge, slotIndex, desired)) return false
    if (desired && slotFocused) host.focusExpandedNeighbor(edge, slotIndex, screen)
    return true
  }

  function toggleCollapsed() {
    return setCollapsed(!collapsed)
  }

  function reportFocus(focused) {
    if (host && !retired && !popout) host.reportSlotFocus(edge, slotIndex, !!focused)
  }

  function handlePressed(item, x, y, tab) {
    if (popout) return
    var point = item.mapToItem(null, x, y)
    handleStartX = point.x
    handleStartY = point.y
    handleDragging = false
    var wanted = Number(tab)
    handleTab = isFinite(wanted) && wanted >= 0 ? wanted : -1
    if (host) host.pressActive = true
  }

  function cycleTab(delta) {
    if (host) host.cycleSlotTab(edge, slotIndex, delta)
  }

  function selectTab(index) {
    if (host) host.setSlotTab(edge, slotIndex, index)
  }

  function openTab(module, state) {
    return host ? host.addTab(edge, slotIndex, String(module || moduleId), state) : false
  }

  function openModulePicker(anchorX) {
    return typeof modulePickerRequested === "function" ? modulePickerRequested(anchorX) : false
  }

  function handleMoved(item, x, y) {
    if (!host || popout) return
    var point = item.mapToItem(null, x, y)
    if (!handleDragging) {
      if (Math.abs(point.x - handleStartX) < host.dragThreshold && Math.abs(point.y - handleStartY) < host.dragThreshold) return
      handleDragging = true
      host.beginSlotDrag(
        edge, slotIndex, screen, dragScope,
        surfaceOriginX + point.x, surfaceOriginY + point.y, point.y,
        hostWindow ? Number(hostWindow.stackHeight) || Number(hostWindow.height) || 0 : 0,
        handleTab
      )
      return
    }
    host.updateSlotDrag(surfaceOriginX + point.x, surfaceOriginY + point.y, point.y)
  }

  function handleReleased(item, x, y) {
    if (popout) {
      requestFocus("")
      return
    }
    if (host) host.pressActive = false
    if (handleDragging && host) {
      handleMoved(item, x, y)
      host.endSlotDrag(true)
    } else {
      requestFocus("")
    }
    handleDragging = false
  }

  function handleCanceled() {
    if (popout) return
    if (host) host.pressActive = false
    if (handleDragging && host) host.endSlotDrag(false)
    handleDragging = false
  }
}
