import QtQuick
import QtTest
import "../../blades" as Blades
import "../../controllers" as Controllers
import "fixtures/core_goblins/Manifest.js" as Goblins

TestCase {
  id: test
  name: "CoreModulesAndGoblins"
  readonly property var names: ["skills", "memory", "hooks", "mcp"]
  readonly property string appRoot: localPath("../../")
  readonly property string goblinsRoot: localPath("fixtures/core_goblins")

  QtObject { id: files }
  QtObject { id: firstView }
  QtObject { id: secondView }

  Blades.BladeRegistry {
    id: moduleRegistry
    socketId: "data-goblin.fileblade/blade"
    contractVersion: 3
  }

  Controllers.ExtensionProviders {
    id: manager
    files: files
    builtinProviders: moduleRegistry.builtinProviders
    inventoryUrl: String(Qt.resolvedUrl("fixtures/CoreInventory.qml"))
  }

  function localPath(path) {
    return decodeURIComponent(String(Qt.resolvedUrl(path)).replace(/^file:\/\//, "").replace(/\/$/, ""))
  }

  function definition(name) {
    return { id: name, name: name, provider: "Provider.qml", entry: "blades/Module.qml", hostContract: 3, singleton: true }
  }

  function init() {
    manager.providers = []
    manager.disclosed = ({})
    moduleRegistry.catalogProviders = []
    var rows = []
    for (var i = 0; i < names.length; i++)
      rows.push({ definition: definition(names[i]), source_dir: appRoot + "/modules/" + names[i], source: "builtin" })
    moduleRegistry.parseScanOutput({ modules: rows })
  }

  function cleanup() {
    manager.shutdown()
    moduleRegistry.fileModules = ({})
  }

  function test_builtins_and_exact_aliases_share_one_lazy_provider() {
    compare(moduleRegistry.order.length, 4)
    compare(Object.keys(manager.services).length, 4)
    for (var i = 0; i < names.length; i++) {
      var name = names[i]
      var definition = moduleRegistry.module(name)
      compare(definition.providerId, "fileblade.core." + name)
      compare(definition, moduleRegistry.module("data-goblin.fileblade-" + name + "/" + name))
      compare(definition, moduleRegistry.module("kurt.agent-" + name + "/" + name))
      var provider = manager.services[definition.providerId]
      compare(provider.providerRoot, appRoot + "/modules/" + name)
      compare(provider.viewCount, 0)
      compare(provider.inventory, null)
      verify(provider.attach(firstView))
      verify(provider.attach(firstView))
      compare(provider.viewCount, 1)
      verify(!!provider.inventory)
      var shared = provider.inventory
      verify(provider.attach(secondView))
      compare(provider.inventory, shared)
      compare(provider.viewCount, 2)
      provider.detach(firstView)
      compare(provider.inventory.observers.length, 1)
      provider.detach(secondView)
      compare(provider.inventory.observers.length, 0)
    }
    compare(moduleRegistry.module("acme.skills/skills"), null)
    compare(moduleRegistry.canonicalModule("kurt.goblin-images/goblin-images"), "kurt.goblin-images/goblin-images")
  }

  function test_goblins_template_provider_survives_next_to_absorbed_companions() {
    var goblins = Goblins.manifest
    compare(goblins.id, "kurt.goblin-images")
    var rows = [{ id: goblins.id, dir: goblinsRoot, manifest: goblins, enabled: true }]
    for (var i = 0; i < names.length; i++) {
      var manifest = { id: "data-goblin.fileblade-" + names[i], extensions: {} }
      manifest.extensions["data-goblin.fileblade/blade"] = [definition(names[i])]
      rows.push({ id: manifest.id, dir: goblinsRoot, manifest: manifest, enabled: true })
    }
    moduleRegistry.catalogProviders = rows
    moduleRegistry.rebuild()
    manager.providers = rows
    compare(moduleRegistry.order.length, 5)
    compare(Object.keys(manager.services).length, 5)
    var found = moduleRegistry.module("kurt.goblin-images/goblin-images")
    verify(!!found)
    compare(found.providerId, goblins.id)
    compare(found.settings.defaults.stackLayers, true)
    var provider = manager.services[found.providerId]
    verify(!!provider)
    compare(provider.providerRoot, goblinsRoot)
    verify(provider.attach(firstView))
    compare(provider.viewCount, 1)
    rows[0] = Object.assign({}, rows[0], { enabled: false })
    manager.providers = rows.slice()
    compare(provider.retired, true)
    compare(manager.services[goblins.id], undefined)
    compare(Object.keys(manager.services).length, 4)
  }

  function test_user_modules_and_external_rows_cannot_claim_core_providers() {
    manager.providers = [{ id: "fileblade.core.skills", dir: goblinsRoot, enabled: true, manifest: Goblins.manifest }]
    compare(manager.services["fileblade.core.skills"].providerRoot, appRoot + "/modules/skills")
    moduleRegistry.parseScanOutput({ modules: [{ definition: definition("skills"), source_dir: goblinsRoot, source: "user" }] })
    compare(moduleRegistry.module("skills").providerId, "")
    compare(moduleRegistry.builtinProviders.length, 0)
    compare(Object.keys(manager.services).length, 0)
  }
}
