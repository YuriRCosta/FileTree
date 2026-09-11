import QtQuick
import Quickshell
import "../../blades" as Blades

ShellRoot {
  id: probe

  Blades.BladeRegistry { id: moduleRegistry; contractVersion: 3 }

  QtObject {
    id: fixture
    property var config: ({})
    property var legacyDefaults: ({})
    property int minimumWidth: 200
    property int slotHandleSize: 10
    property var layout: ({ left: { slots: [] }, right: { slots: [] } })
    property var edges: ["left", "right"]
    property var registry: moduleRegistry
    property var activeSlots: ({ left: 0, right: 0 })
    property var windowAddresses: ({})
    property string monitorMode: "all"
    property string monitorLock: ""
    property bool animateBlades: false
  }

  Blades.BladeLayout { id: layout; host: fixture }

  function require(value, message) {
    if (!value) throw new Error(message)
  }

  function run() {
    var names = ["skills", "memory", "hooks", "mcp"]
    for (var i = 0; i < names.length; i++) {
      var name = names[i]
      var legacy = "data-goblin.fileblade-" + name + "/" + name
      var slot = layout.normalizeSlot({ id: "saved", modules: [{ module: legacy, state: { query: "custom", future: 7 } }], active: 0 }, 0, {})
      require(slot.modules[0].module === name, "slot alias " + name)
      require(slot.modules[0].state.future === 7 && slot.modules[0].state.query === "custom", "slot state " + name)
      fixture.layout = ({ left: { slots: [slot] }, right: { slots: [] } })
      require(layout.findModule(name).index === 0, "canonical singleton " + name)
      require(layout.findModule(legacy).index === 0, "legacy singleton " + name)
      require(layout.findModule("kurt.agent-" + name + "/" + name).index === 0, "older singleton " + name)
    }
    var goblins = "kurt.goblin-images/goblin-images"
    var mixed = layout.normalizeSlot({ id: "mixed", modules: [{ module: "data-goblin.fileblade-mcp/mcp", state: { query: "saved" } }, { module: goblins, state: { stackLayers: false } }], active: 1 }, 0, {})
    require(mixed.active === 1 && mixed.modules[1].module === goblins, "Goblins active tab")
    require(mixed.modules[1].state.stackLayers === false && mixed.modules[0].state.query === "saved", "mixed tab state")
    console.log("CORE_SLOTS_PASS")
    Qt.quit()
  }

  Component.onCompleted: Qt.callLater(function() {
    try { probe.run() }
    catch (error) { console.error("CORE_SLOTS_FAIL " + error); Qt.quit() }
  })
}
