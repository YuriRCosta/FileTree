import "../../ui"

InventoryProvider {
  inventoryOptions: ({
    maximumItems: 1024, itemsKey: "definitions", healthBasis: "configuration-only",
    exactProject: false, scanArguments: ["--watch"],
    activityMethod: "usage", activityArguments: function() { return ["--json"] }
  })
}
