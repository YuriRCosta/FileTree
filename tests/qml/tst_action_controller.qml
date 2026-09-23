import QtQuick
import QtTest
import "../../controllers" as Controllers

TestCase {
  id: testCase
  name: "ActionControllerRegression"
  property var controller: null

  QtObject {
    id: fakeRegistry
    property var installedPlugins: ({})
    function isEnabled(plugin) { return plugin !== "disabled.plugin" }
  }

  QtObject {
    id: fakeBladeRegistry
  }

  QtObject {
    id: fakeBladeHost
    property string configDir: "/tmp/filetree-test"
    property var registry: fakeBladeRegistry
  }

  QtObject {
    id: fakeService
    property string rootPath: "/tmp"
    property string operationError: ""
    property string operationNotice: ""
    property var pluginRegistry: fakeRegistry
    property var bladeHost: fakeBladeHost
    property var requests: []
    property var statCallback: null
    property var runCallback: null

    function backendRequest(name, arguments, generation, callback, progress, deadline) {
      requests = requests.concat([{ name: name, arguments: arguments, deadline: deadline }])
      if (name === "stat-batch") statCallback = callback
      if (name === "action-run") runCallback = callback
      return name + "-" + requests.length
    }
    function decodedJsonDocument(text) {
      var source = String(text || "")
      if (source.indexOf("base64:") === 0) source = Qt.atob(source.slice(7))
      try { return { ok: true, value: JSON.parse(source) } }
      catch (error) { return { ok: false, value: null } }
    }
  }

  Component {
    id: controllerComponent
    Controllers.ActionController {}
  }

  function action(extra) {
    var row = {
      key: "test.plugin/dump",
      id: "dump",
      source: "plugin",
      plugin: "test.plugin",
      pluginRoot: "/tmp/test.plugin",
      title: "Dump",
      glyph: "x",
      description: "",
      contexts: ["selection"],
      confirm: false,
      detach: false,
      output: "notice",
      timeout: 60,
      program: "scripts/dump"
    }
    if (extra) for (var key in extra) row[key] = extra[key]
    return row
  }

  function init() {
    fakeService.requests = []
    fakeService.statCallback = null
    fakeService.runCallback = null
    fakeService.operationError = ""
    fakeService.operationNotice = ""
    controller = controllerComponent.createObject(testCase, { service: fakeService })
    verify(controller)
    tryVerify(function() { return fakeService.requests.length === 1 })
    compare(fakeService.requests[0].name, "action-list")
    fakeService.requests = []
    controller.applyList({ ok: true, actions: [action()] })
  }

  function cleanup() {
    if (controller) controller.destroy()
    controller = null
  }

  function test_a_catalog_only_provider_is_enabled_and_reaches_the_gate() {
    controller.catalogProviders = [{ id: "acme.actions", dir: "/plugins/acme.actions", manifest: {}, enabled: true }]
    compare(controller.providerEnabled("acme.actions"), true)
    controller.catalogProviders = [{ id: "acme.actions", dir: "/plugins/acme.actions", manifest: {}, enabled: false }]
    compare(controller.providerEnabled("acme.actions"), false)
    compare(controller.gate({ source: "plugin", plugin: "acme.actions", key: "acme.actions/run" }, null, true, 1),
            "plugin acme.actions is disabled")
    controller.catalogProviders = []
  }

  function test_menu_refuses_a_selection_instead_of_running_a_prefix() {
    var entries = []
    for (var i = 0; i < 257; i++) entries.push({ path: "/tmp/f" + i, isDir: false })
    compare(controller.run("test.plugin/dump", entries, "/tmp", null, true), "")
    compare(fakeService.operationError, "selection too large (256)")
    compare(fakeService.requests.length, 0)
  }

  function test_ipc_claim_survives_a_second_request_and_completes() {
    var first = controller.runFromIpc("test.plugin/dump", '["/tmp/a"]', true)
    verify(first.ok)
    compare(fakeService.requests[0].name, "stat-batch")
    var second = controller.runFromIpc("test.plugin/dump", '["/tmp/b"]', true)
    verify(!second.ok)
    compare(second.error, "already running")
    compare(controller.result(first.request_id).status, "running")

    fakeService.statCallback({ ok: true, entries: [{ path: "/tmp/a", isDir: false }] })
    compare(fakeService.requests[1].name, "action-run")
    compare(controller.result(first.request_id).status, "running")
    fakeService.runCallback({ ok: true, exit_code: 0, elapsed_ms: 1, stdout_tail: "" })
    compare(controller.result(first.request_id).status, "done")
    compare(controller.running["test.plugin/dump"], undefined)
  }

  function test_ipc_bounds_paths_results_and_list_errors() {
    verify(!controller.runFromIpc("test.plugin/dump", '[""]', true).ok)
    verify(!controller.runFromIpc("test.plugin/dump", '["a\\u0000b"]', true).ok)
    var huge = "[\"" + "x".repeat(65536) + "\"]"
    compare(controller.runFromIpc("test.plugin/dump", huge, true).error, "path list is too large")
    compare(controller.result("x".repeat(1000)).id.length, 128)
    controller.applyList({
      ok: true,
      actions: [action()],
      errors: [{ source: "test.plugin", error: "bad action\nignored" }]
    })
    compare(controller.document().errors, [{ source: "test.plugin", error: "bad action" }])
  }

  function test_ipc_decodes_a_base64_path_document() {
    var started = controller.runFromIpc("test.plugin/dump", "base64:WyIvdG1wL2EiXQ==", true)
    verify(started.ok)
    compare(fakeService.requests[0].name, "stat-batch")
    compare(fakeService.requests[0].arguments, ["--path", "/tmp/a"])
  }
}
