import "../../ui"

InventoryProvider {
  inventoryOptions: ({
    maximumItems: 256, activityMethod: "usage",
    activityArguments: function(inventory) { return ["--json", "--project", inventory.anchorPath].concat(inventory.projectArguments) }
  })
}
