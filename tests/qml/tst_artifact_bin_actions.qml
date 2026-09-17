import QtQuick
import QtTest
import "../../controllers"
import "../../ui"

TestCase {
  id: test
  name: "ArtifactBinActions"
  property var requests: []
  property int changes: 0
  property var actions: null
  property var bin: null
  property bool consent: true

  Item {
    id: files
    property bool agentManagementEnabled: test.consent
    readonly property string cliPath: "/usr/bin/fileblade"
    property var artifactActions: test.actions
    function backendCommand(name) { return [cliPath, "_backend", String(name)] }
    function backendRequest(command, args, generation, callback, progress, deadline, options) {
      var id = "request-" + test.requests.length
      test.requests.push({ id: id, command: String(command), args: args, callback: callback })
      return id
    }
    function cancelBackendRequest(id, generation, discard) { return true }
    function refreshTrash() { return true }
  }

  Component { id: controller; ArtifactActionController { service: files } }
  Component { id: component; ArtifactBin { service: files } }

  function dispatches() {
    return requests.filter(function(request) { return request.command !== "bin-list" })
  }

  function keys() {
    return bin.choices.map(function(option) { return String(option.key) })
  }

  function argument(request, name) {
    var index = request.args.indexOf(name)
    return index >= 0 && index + 1 < request.args.length ? String(request.args[index + 1]) : ""
  }

  function complete(response) {
    var pending = dispatches()
    var request = pending[pending.length - 1]
    request.callback(response)
    wait(0)
  }

  function open(module, route, entry) {
    bin.module = module
    bin.helperRoute = route
    bin.describe = function(row) { return row.item !== undefined ? row.item : null }
    bin.removalArguments = function(row) { return ["--project", "/project", "--id", String(row.id), "--json"] }
    bin.changed.connect(function() { test.changes++ })
    bin.ask(entry)
  }

  function skillRow() {
    return { id: "skill-1", name: "Fixture", kind: "skill",
             item: { id: "skill-1", name: "Fixture", kind: "skill", scope: "user",
                     path: "/home/agent/.claude/skills/fixture", isDir: true,
                     paths: ["/home/agent/.claude/skills/fixture"], skillRoot: true } }
  }

  function definitionRow(module) {
    return { id: module + "-1", name: "Fixture", kind: module,
             item: { id: module + "-1", name: "Fixture", kind: module, scope: "project",
                     path: "/project/.mcp.json", paths: [] } }
  }

  function init() {
    requests = []
    changes = 0
    consent = true
    actions = createTemporaryObject(controller, test)
    bin = createTemporaryObject(component, test)
    verify(actions !== null && bin !== null)
  }

  function cleanup() {
    if (bin) bin.destroy()
    bin = null
    if (actions) actions.destroy()
    actions = null
    wait(0)
  }

  function test_a_skill_offers_every_choice_without_a_separate_consent() {
    var row = skillRow()
    open("skills", null, row)
    compare(bin.error, "")
    compare(keys(), ["cancel", "trash", "bin"])
    compare(dispatches().length, 0)
  }

  function test_skill_removal_moves_the_folder_into_the_recoverable_bin() {
    open("skills", null, skillRow())
    compare(keys(), ["cancel", "trash", "bin"])
    bin.choose("bin")
    compare(dispatches().length, 1)
    var request = dispatches()[0]
    compare(request.command, "bin-put")
    compare(argument(request, "--module"), "skills")
    compare(JSON.parse(argument(request, "--item")).path, "/home/agent/.claude/skills/fixture")
    complete({ ok: true, entry: "bin:1" })
    verify(bin.error === "")
  }

  function test_skill_trash_follows_symlinks_and_carries_every_described_path() {
    open("skills", null, skillRow())
    bin.choose("trash")
    var request = dispatches()[0]
    compare(request.command, "trash")
    verify(request.args.indexOf("--follow-symlinks") >= 0)
    compare(argument(request, "--path"), "/home/agent/.claude/skills/fixture")
  }

  function test_a_symlinked_skill_offers_deleting_only_the_link() {
    var row = skillRow()
    row.item.linkTarget = "/home/agent/vault/skills/fixture"
    row.linkTarget = "/home/agent/vault/skills/fixture"
    open("skills", null, row)
    compare(keys(), ["cancel", "trash", "unlink", "bin"])
    compare(bin.choices.map(function(option) { return String(option.label) }), ["Cancel", "Delete skill", "Delete symlink", "Deactivate"])
    bin.choose("unlink")
    var request = dispatches()[0]
    compare(request.command, "trash")
    verify(request.args.indexOf("--follow-symlinks") < 0)
    compare(argument(request, "--path"), "/home/agent/.claude/skills/fixture")
    complete({ ok: true })
    var binned = { id: "bin:2", name: "Fixture", kind: "bin", groups: ["Trash"], linkTarget: "/home/agent/vault/skills/fixture" }
    bin.ask(binned)
    compare(bin.choices.map(function(option) { return String(option.label) }), ["Cancel", "Delete symlink forever", "Restore"])
  }

  function test_memory_removal_uses_the_path_bin_without_a_helper_route() {
    var row = { id: "memory-1", name: "AGENTS.md",
                item: { id: "memory-1", name: "AGENTS.md", kind: "memory", scope: "project",
                        path: "/project/AGENTS.md", paths: ["/project/AGENTS.md"] } }
    open("memory", null, row)
    compare(keys(), ["cancel", "trash", "bin"])
    bin.choose("bin")
    compare(dispatches()[0].command, "bin-put")
    compare(argument(dispatches()[0], "--module"), "memory")
  }

  function test_definitions_inside_shared_files_offer_no_trash_choice() {
    var route = { provider: "fileblade.core.hooks", directory: "", helper: "inventory" }
    open("hooks", route, definitionRow("hooks"))
    compare(keys(), ["cancel", "bin"])
  }

  function test_definition_removal_routes_through_its_helper() {
    var route = { provider: "fileblade.core.mcp", directory: "", helper: "inventory" }
    open("mcp", route, definitionRow("mcp"))
    bin.choose("bin")
    var request = dispatches()[0]
    compare(request.command, "bin-remove")
    compare(argument(request, "--module"), "mcp")
    compare(JSON.parse(argument(request, "--helper-route")).provider, "fileblade.core.mcp")
    compare(JSON.parse(argument(request, "--arguments"))[3], "mcp-1")
  }

  function test_a_forced_trash_of_a_shared_definition_explains_itself_and_dispatches_nothing() {
    var route = { provider: "fileblade.core.hooks", directory: "", helper: "inventory" }
    var row = definitionRow("hooks")
    open("hooks", route, row)
    changes = 0
    bin.pending = row
    bin.choose("trash")
    compare(dispatches().length, 0)
    compare(changes, 1)
    verify(bin.error !== "")
  }

  function test_a_vanished_item_refuses_instead_of_removing_nothing() {
    var row = definitionRow("hooks")
    open("hooks", { provider: "fileblade.core.hooks", directory: "", helper: "inventory" }, row)
    bin.describe = function(entry) { return null }
    changes = 0
    bin.choose("bin")
    compare(dispatches().length, 0)
    compare(changes, 1)
    verify(bin.error !== "")
  }

  function test_binned_rows_offer_restore_and_permanent_discard() {
    var entry = { id: "bin:1", name: "Fixture", kind: "bin", groups: ["Trash"] }
    open("skills", null, entry)
    compare(keys(), ["cancel", "purge", "restore"])
    bin.choose("restore")
    compare(dispatches()[0].command, "bin-restore")
    compare(argument(dispatches()[0], "--id"), "bin:1")
    complete({ ok: true })
    bin.ask(entry)
    bin.choose("purge")
    compare(dispatches()[1].command, "bin-purge")
    compare(argument(dispatches()[1], "--id"), "bin:1")
  }

  function test_a_failed_removal_reports_the_backend_message() {
    open("hooks", { provider: "fileblade.core.hooks", directory: "", helper: "inventory" }, definitionRow("hooks"))
    bin.choose("bin")
    complete({ ok: false, message: "the source definition changed; refresh and retry" })
    compare(bin.error, "the source definition changed; refresh and retry")
  }
}
