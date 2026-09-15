import QtQuick
import QtTest
import "../../lib/LayoutInventory.js" as LayoutInventory

TestCase {
  name: "LayoutInventory"

  readonly property var saved: ({
    blades: {
      left: { slots: [{ id: "a", modules: [{ module: "files" }] },
                      { id: "b", modules: [{ module: "properties" }] }] },
      right: { slots: [{ id: "c", modules: [{ module: "skills" }, { module: "hooks" }] }] }
    }
  })

  function test_inventory_reads_every_module_the_document_names() {
    compare(LayoutInventory.modules(saved).join(","), "files,hooks,properties,skills")
    compare(LayoutInventory.modules({}).length, 0)
    compare(LayoutInventory.modules({ blades: { left: { slots: [{ module: "notes" }] } } }).join(","), "notes")
    compare(LayoutInventory.modules({ blades: { left: { slots: [{ modules: [{ module: "notes" }, { module: "notes" }] }] } } }).join(","), "notes")
  }

  function test_a_restore_that_lost_modules_names_them() {
    var kept = { left: { slots: [{ modules: [{ module: "files" }] }] }, right: { slots: [] } }
    compare(LayoutInventory.missing(saved, kept).join(","), "hooks,properties,skills")
  }

  function test_a_complete_restore_loses_nothing() {
    var kept = {
      left: { slots: [{ modules: [{ module: "files" }] }, { modules: [{ module: "properties" }] }] },
      right: { slots: [{ modules: [{ module: "skills" }, { module: "hooks" }] }] }
    }
    compare(LayoutInventory.missing(saved, kept).length, 0)
  }

  function test_an_alias_counts_as_kept() {
    var kept = { left: { slots: [{ modules: [{ module: "file-tree" }] }] }, right: { slots: [] } }
    var resolve = function(name) { return name === "files" ? "file-tree" : name }
    compare(LayoutInventory.missing({ blades: { left: { slots: [{ module: "files" }] } } }, kept, resolve).length, 0)
  }
}
