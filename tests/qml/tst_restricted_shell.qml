import QtQuick
import QtTest
import "../../blades" as Blades
import "../../controllers" as Controllers

TestCase {
  id: testCase
  name: "RestrictedShellExtensions"

  QtObject {
    id: selfScopedRegistry
    property var installedPlugins: ({ "data-goblin.fileblade": { id: "data-goblin.fileblade" } })
    property var enabledIds: ["data-goblin.fileblade"]
    function isEnabled(id) { return enabledIds.indexOf(String(id)) >= 0 }
  }

  QtObject {
    id: fakeService
    property var lastRequest: null
    property var reply: null
    property var watchEvent: null
    property var watchPaths: []
    property int requestCount: 0
    property bool defer: false
    property var pending: null
    function backendRequest(name, args, generation, callback) {
      requestCount++
      lastRequest = { name: name, args: args }
      if (defer) {
        pending = callback
        return "request-1"
      }
      if (callback) callback(reply)
      return "request-1"
    }
    function settle() {
      var callback = pending
      pending = null
      if (callback) callback(reply)
    }
    property var watchReady: null
    property var watchClosed: null
    function backendSubscribe(paths, generation, event, ready, closed) {
      watchPaths = paths
      watchEvent = event
      watchReady = ready
      watchClosed = closed
      return "watch-1"
    }
    function emitWatchReady() { if (watchReady) watchReady({ ok: true }) }
    function emitWatchClosed() { if (watchClosed) watchClosed({ ok: false }) }
    function cancelBackendRequest(id, generation, discard) { watchEvent = null }
    function emitWatchEvent() { if (watchEvent) watchEvent({ path: "/home/tester/.config/omarchy/plugins/acme.one" }) }
  }

  property var companionManifest: ({
    id: "acme.fileblade-weather",
    extensions: {
      "data-goblin.fileblade/blade": [{
        hostContract: 2,
        id: "weather",
        name: "Weather",
        entry: "blades/Module.qml",
        provider: "Provider.qml"
      }]
    }
  })

  Blades.BladeRegistry {
    id: registry
    socketId: "data-goblin.fileblade/blade"
    pluginDir: "/opt/fileblade"
    contractVersion: 2
    pluginRegistry: selfScopedRegistry
  }

  Controllers.ExtensionCatalog {
    id: catalog
    service: fakeService
  }

  Controllers.ExtensionProviders {
    id: providers
    inventoryUrl: "file:///opt/fileblade/ui/ArtifactInventory.qml"
    files: fakeService
  }

  function test_a_self_scoped_registry_hides_every_companion() {
    registry.catalogProviders = []
    var hidden = registry.manifestModules()
    compare(Object.keys(hidden).length, 0)
  }

  function test_the_catalog_restores_companion_modules_and_their_source() {
    registry.catalogProviders = [{
      id: "acme.fileblade-weather",
      dir: "/home/tester/.config/omarchy/plugins/acme.fileblade-weather",
      manifest: companionManifest,
      enabled: true
    }]
    var modules = registry.manifestModules()
    var ids = Object.keys(modules)
    compare(ids.length, 1)
    compare(ids[0], "acme.fileblade-weather/weather")
    var module = modules[ids[0]]
    compare(module.sourceDir, "/home/tester/.config/omarchy/plugins/acme.fileblade-weather")
    compare(String(module.entryUrl).indexOf("/acme.fileblade-weather/blades/Module.qml") >= 0, true)
    compare(module.providerId, "acme.fileblade-weather")
    compare(module.providerEntry, "Provider.qml")
    compare(module.compatible, true)
  }

  function test_a_disabled_companion_is_listed_as_disabled_not_loaded() {
    registry.catalogProviders = [{
      id: "acme.fileblade-weather",
      dir: "/home/tester/.config/omarchy/plugins/acme.fileblade-weather",
      manifest: companionManifest,
      enabled: false
    }]
    var modules = registry.manifestModules()
    compare(Object.keys(modules).length, 0)
    compare(Object.keys(registry.disabledModules).length, 1)
  }

  function test_the_registry_wins_when_the_shell_still_discloses_a_companion() {
    selfScopedRegistry.installedPlugins = ({
      "data-goblin.fileblade": { id: "data-goblin.fileblade" },
      "acme.fileblade-weather": Object.assign({ __sourceDir: "/shell/route" }, companionManifest)
    })
    selfScopedRegistry.enabledIds = ["data-goblin.fileblade", "acme.fileblade-weather"]
    registry.catalogProviders = [{
      id: "acme.fileblade-weather",
      dir: "/disk/route",
      manifest: companionManifest,
      enabled: true
    }]
    var modules = registry.manifestModules()
    var ids = Object.keys(modules)
    compare(ids.length, 1)
    compare(modules[ids[0]].sourceDir, "/shell/route")
    selfScopedRegistry.installedPlugins = ({ "data-goblin.fileblade": { id: "data-goblin.fileblade" } })
    selfScopedRegistry.enabledIds = ["data-goblin.fileblade"]
  }

  function test_the_catalog_accepts_only_complete_provider_rows() {
    compare(catalog.accepted(null), null)
    compare(catalog.accepted({ ok: false, providers: [] }), null)
    compare(catalog.accepted({ ok: true }), null)
    var rows = catalog.accepted({ ok: true, providers: [
      { id: "acme.one", dir: "/plugins/acme.one", manifest: companionManifest, enabled: true },
      { id: "", dir: "/plugins/nameless", manifest: companionManifest, enabled: true },
      { id: "acme.two", dir: "", manifest: companionManifest, enabled: true },
      { id: "acme.three", dir: "/plugins/acme.three", enabled: true }
    ]})
    compare(rows.length, 1)
    compare(rows[0].id, "acme.one")
    compare(rows[0].enabled, true)
  }

  function test_a_refresh_asks_the_backend_for_the_catalog() {
    catalog.checkedAt = 0
    catalog.dirty = false
    fakeService.reply = { ok: true, activation: "known", providers: [
      { id: "acme.one", dir: "/plugins/acme.one", manifest: companionManifest, enabled: true }
    ]}
    catalog.refresh()
    compare(fakeService.lastRequest.name, "plugin-catalog")
    compare(catalog.activation, "known")
    compare(catalog.providers.length, 1)
    compare(catalog.error, "")
  }

  function test_an_unreadable_catalog_keeps_the_rows_but_drops_their_authority() {
    fakeService.reply = { ok: true, activation: "known", providers: [
      { id: "acme.one", dir: "/plugins/acme.one", manifest: companionManifest, enabled: true }
    ]}
    catalog.checkedAt = 0
    catalog.refresh()
    compare(catalog.providers[0].enabled, true)
    fakeService.reply = { ok: false, message: "no plugins directory" }
    catalog.checkedAt = 0
    catalog.refresh()
    compare(catalog.providers.length, 1)
    compare(catalog.providers[0].enabled, false)
    compare(catalog.activation, "unknown")
    compare(catalog.error, "no plugins directory")
  }

  function test_an_unknown_activation_never_reads_as_enabled() {
    fakeService.reply = { ok: true, activation: "unknown", providers: [
      { id: "acme.one", dir: "/plugins/acme.one", manifest: companionManifest, enabled: true }
    ]}
    catalog.checkedAt = 0
    catalog.refresh()
    compare(catalog.providers.length, 1)
    compare(catalog.providers[0].enabled, false)
  }

  function test_a_watch_that_comes_back_resets_its_backoff() {
    fakeService.reply = { ok: true, activation: "known", providers: [] }
    catalog.watchPaths = ["/home/tester/.config/omarchy", "/home/tester/.config/omarchy/plugins"]
    catalog.watchRequestId = ""
    catalog.watchFailures = 3
    catalog.checkedAt = 0
    catalog.watch()
    fakeService.emitWatchReady()
    compare(catalog.watchFailures, 0)
    fakeService.emitWatchClosed()
    compare(catalog.watchRequestId, "")
    compare(catalog.watchFailures, 1)
    compare(catalog.recovery.running, true)
    compare(catalog.activation, "unknown")
  }

  function test_only_a_plugin_or_shell_change_wakes_the_catalog() {
    compare(catalog.relevant({ path: "/home/tester/.config/omarchy/plugins/acme.one" }), true)
    compare(catalog.relevant({ path: "/home/tester/.config/omarchy/shell.json" }), true)
    compare(catalog.relevant({ overflow: true }), true)
    compare(catalog.relevant({ path: "/home/tester/.config/omarchy/current/theme" }), false)
    compare(catalog.relevant({}), false)
    compare(catalog.relevant(null), false)
  }

  function test_native_activation_and_first_extension_directory_wake_the_catalog() {
    var original = catalog.watchPaths
    var originalProviders = catalog.providers
    catalog.providers = []
    catalog.watchPaths = ["/config", "/config/fileblade", "/config/fileblade/extensions", "/config/omarchy/fileblade"]
    compare(catalog.relevant({ path: "/config/fileblade/extensions/acme.one" }), true)
    compare(catalog.relevant({ path: "/config/omarchy/fileblade/settings.json" }), true)
    compare(catalog.relevant({ path: "/config/omarchy/fileblade/blades.json" }), false)
    compare(catalog.relevant({ path: "/config/omarchy/plugins/acme.one" }), false)
    compare(catalog.relevant({ path: "/config/omarchy/shell.json" }), false)
    var before = catalog.generation
    compare(catalog.relevant({ path: "/config/fileblade" }), true)
    catalog.watch()
    verify(catalog.generation > before)
    compare(fakeService.watchPaths, catalog.watchPaths)
    var subscribedGeneration = catalog.generation
    compare(catalog.relevant({ path: "/config/fileblade/extensions/acme.late", directory: true, events: ["create"] }), true)
    compare(catalog.generation, subscribedGeneration)
    fakeService.reply = { ok: true, activation: "known", providers: [] }
    catalog.read()
    compare(catalog.relevant({ path: "/config/fileblade/extensions/acme.sibling", directory: true, events: ["create"] }), true)
    compare(catalog.generation, subscribedGeneration)
    tryVerify(function() { return fakeService.watchPaths.indexOf("/config/fileblade/extensions/acme.sibling") >= 0 })
    verify(fakeService.watchPaths.indexOf("/config/fileblade/extensions/acme.late") >= 0)
    subscribedGeneration = catalog.generation
    compare(catalog.watch(), false)
    compare(catalog.generation, subscribedGeneration)
    compare(catalog.relevant({ path: "/config/fileblade/extensions/acme.late/manifest.json" }), true)
    compare(catalog.relevant({ path: "/config/fileblade/extensions/acme.late/Module.qml" }), true)
    compare(catalog.relevant({ path: "/config/fileblade/extensions/acme.late", directory: true, events: ["delete"] }), true)
    catalog.watch()
    verify(fakeService.watchPaths.indexOf("/config/fileblade/extensions/acme.late") < 0)
    var retiredEvent = fakeService.watchEvent
    catalog.watchSignature = ""
    catalog.watch()
    retiredEvent({ path: "/config/fileblade/extensions/acme.handoff", directory: true, events: ["create"] })
    verify(catalog.extensionDirectories.indexOf("/config/fileblade/extensions/acme.handoff") < 0)
    fakeService.reply = { ok: true, activation: "known", providers: [], directory_names: ["acme.handoff", "../escape", "a/b", "", ".hidden", null] }
    catalog.checkedAt = 0
    fakeService.emitWatchReady()
    tryVerify(function() { return fakeService.watchPaths.indexOf("/config/fileblade/extensions/acme.handoff") >= 0 })
    compare(catalog.providers.length, 0)
    verify(catalog.extensionDirectories.every(function(path) { return path.indexOf("/config/fileblade/extensions/acme.") === 0 }))
    fakeService.reply = { ok: true, activation: "known", providers: [{ id: "acme.handoff", dir: "/config/fileblade/extensions/acme.handoff", manifest: companionManifest, enabled: true }], directory_names: ["acme.handoff"] }
    catalog.checkedAt = 0
    fakeService.watchEvent({ path: "/config/fileblade/extensions/acme.handoff/Module.qml", events: ["close_write"] })
    compare(catalog.providers.length, 1)
    compare(catalog.providers[0].id, "acme.handoff")
    compare(catalog.providers[0].enabled, true)
    catalog.unwatch()
    catalog.extensionDirectories = []
    catalog.watchPaths = original
    catalog.providers = originalProviders
  }

  function test_a_request_during_a_read_is_kept_and_served_afterwards() {
    fakeService.reply = { ok: true, activation: "known", providers: [] }
    fakeService.defer = true
    catalog.checkedAt = 0
    catalog.dirty = false
    compare(catalog.requestRefresh(), true)
    compare(catalog.loading, true)
    compare(catalog.requestRefresh(), false)
    compare(catalog.dirty, true)
    var before = fakeService.requestCount
    fakeService.settle()
    compare(catalog.loading, false)
    compare(catalog.dirty, true)
    compare(catalog.trailing.running, true)
    catalog.checkedAt = Date.now() - catalog.minimumIntervalMs - 1
    compare(catalog.pump(), true)
    compare(fakeService.requestCount, before + 1)
    compare(catalog.dirty, false)
    fakeService.defer = false
    fakeService.settle()
  }

  function test_a_kept_request_survives_a_throttled_wake_up() {
    fakeService.reply = { ok: true, activation: "known", providers: [] }
    catalog.checkedAt = Date.now()
    catalog.dirty = false
    compare(catalog.requestRefresh(), false)
    compare(catalog.dirty, true)
    var before = fakeService.requestCount
    catalog.pump()
    compare(fakeService.requestCount, before)
    compare(catalog.dirty, true)
    catalog.checkedAt = Date.now() - catalog.minimumIntervalMs - 1
    compare(catalog.pump(), true)
    compare(fakeService.requestCount, before + 1)
    compare(catalog.dirty, false)
  }

  function test_an_event_inside_the_window_refreshes_once_afterwards() {
    fakeService.reply = { ok: true, activation: "known", providers: [] }
    catalog.checkedAt = 0
    compare(catalog.refresh(), true)
    var before = fakeService.requestCount
    compare(catalog.requestRefresh(), false)
    compare(fakeService.requestCount, before)
    compare(catalog.trailing.running, true)
  }

  function test_a_repeat_refresh_is_coalesced_but_a_watch_event_is_not() {
    fakeService.reply = { ok: true, activation: "known", providers: [] }
    catalog.checkedAt = 0
    compare(catalog.refresh(), true)
    compare(catalog.refresh(), false)
    catalog.watchPaths = ["/home/tester/.config/omarchy/shell.json", "/home/tester/.config/omarchy/plugins"]
    compare(catalog.watch(), true)
    compare(fakeService.watchPaths.length, 2)
    catalog.checkedAt = 0
    fakeService.emitWatchEvent()
    compare(fakeService.lastRequest.name, "plugin-catalog")
    compare(catalog.relevant({ path: "/plugins" }), true)
  }

  function test_the_provider_manager_refuses_an_unsafe_or_absent_provider_entry() {
    compare(providers.providerEntry(companionManifest), "Provider.qml")
    compare(providers.providerEntry({ extensions: { "data-goblin.fileblade/blade": [{ provider: "/etc/passwd" }] } }), "")
    compare(providers.providerEntry({ extensions: { "data-goblin.fileblade/blade": [{ provider: "../escape/Provider.qml" }] } }), "")
    compare(providers.providerEntry({ extensions: { "data-goblin.fileblade/blade": [{ provider: null }] } }), "")
    compare(providers.providerEntry({ extensions: { "data-goblin.fileblade/blade": [{ id: "weather" }] } }), "")
    compare(providers.providerEntry(null), "")
  }

  readonly property string fixtureRoot: decodeURIComponent(String(Qt.resolvedUrl("fixtures/provider")).replace(/^file:\/\//, ""))

  function providerRow(id, enabled) {
    return { id: id, dir: fixtureRoot, manifest: companionManifest, enabled: enabled !== false }
  }

  function test_a_declared_provider_is_built_once_and_shared() {
    providers.disclosed = ({})
    providers.providers = [providerRow("acme.one")]
    var runtime = providers.services["acme.one"]
    verify(!!runtime)
    compare(runtime.providerId, "acme.one")
    compare(runtime.providerRoot, fixtureRoot)
    compare(String(runtime.inventoryComponentUrl), providers.inventoryUrl)
    compare(runtime.viewCount, 0)
    providers.providers = [providerRow("acme.one")]
    compare(providers.services["acme.one"], runtime)
    compare(Object.keys(providers.errors).length, 0)
  }

  function test_a_changed_root_replaces_the_runtime_and_shuts_the_old_one_down() {
    providers.disclosed = ({})
    providers.providers = [providerRow("acme.one")]
    var first = providers.services["acme.one"]
    verify(!!first)
    providers.providers = [{ id: "acme.one", dir: "/nowhere/acme.one", manifest: companionManifest, enabled: true }]
    compare(first.retired, true)
    compare(providers.services["acme.one"], undefined)
    compare(providers.errors["acme.one"], "The extension's provider could not be loaded")
  }

  function test_a_shell_disclosed_plugin_keeps_its_own_runtime() {
    providers.disclosed = ({ "acme.one": { id: "acme.one" } })
    providers.providers = [providerRow("acme.one")]
    compare(Object.keys(providers.services).length, 0)
    providers.disclosed = ({})
  }

  function test_a_disclosure_change_alone_hands_the_runtime_back() {
    providers.disclosed = ({})
    providers.providers = [providerRow("acme.one")]
    var runtime = providers.services["acme.one"]
    verify(!!runtime)
    providers.disclosed = ({ "acme.one": { id: "acme.one" } })
    compare(runtime.retired, true)
    compare(Object.keys(providers.services).length, 0)
    providers.disclosed = ({})
    verify(!!providers.services["acme.one"])
    verify(providers.services["acme.one"] !== runtime)
  }

  function test_shutting_down_retires_every_runtime() {
    providers.disclosed = ({})
    providers.providers = [providerRow("acme.one")]
    var runtime = providers.services["acme.one"]
    verify(!!runtime)
    providers.shutdown()
    compare(runtime.retired, true)
    compare(Object.keys(providers.services).length, 0)
  }

  function test_a_disabled_or_rootless_provider_gets_no_runtime() {
    providers.providers = [
      { id: "acme.off", dir: "/plugins/acme.off", manifest: companionManifest, enabled: false },
      { id: "acme.rootless", dir: "", manifest: companionManifest, enabled: true }
    ]
    compare(Object.keys(providers.services).length, 0)
  }
}
