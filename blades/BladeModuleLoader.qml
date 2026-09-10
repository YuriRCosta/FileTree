import QtQuick

Loader {
  id: loader
  required property var liveContext
  property var moduleContext: null

  function retire() {
    if (!moduleContext || moduleContext.retired) return
    // Let the outgoing view flush its original tab before rejecting late work.
    moduleContext.bladeOpen = false
    moduleContext.retired = true
  }

  function loadModule(url) {
    retire()
    setSource("")
    if (moduleContext) moduleContext.destroy()
    moduleContext = null
    if (!url) return
    moduleContext = contextComponent.createObject(loader, {
      edge: liveContext.edge, slotIndex: liveContext.slotIndex,
      slotId: liveContext.slotId, tabIndex: liveContext.tabIndex,
      moduleId: liveContext.moduleId, moduleDir: liveContext.moduleDir,
      providerId: liveContext.providerId
    })
    moduleContext.definition = liveContext.definition
    setSource(url, { context: moduleContext })
  }

  Component.onDestruction: retire()

  Component {
    id: contextComponent
    BladeContext {
      host: loader.liveContext.host
      shell: loader.liveContext.shell
      services: loader.liveContext.services
      hostWindow: loader.liveContext.hostWindow
      screen: loader.liveContext.screen
      tabCount: loader.liveContext.tabCount
      bladeOpen: !retired && loader.liveContext.bladeOpen
      bladeFocused: !retired && loader.liveContext.bladeFocused
      slotFocused: !retired && loader.liveContext.slotFocused
      collapsed: loader.liveContext.collapsed
      slotState: {
        var index = host ? host.findSlotId(edge, slotId) : -1
        var tabs = index >= 0 ? host.slotTabs(edge, index) : []
        var tab = tabs[tabIndex]
        return tab && tab.module === moduleId ? tab.state || ({}) : ({})
      }
      modulePickerRequested: retired ? null : loader.liveContext.modulePickerRequested
    }
  }
}
