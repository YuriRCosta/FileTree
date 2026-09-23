import QtQuick
import QtTest
import "../../controllers" as Controllers

TestCase {
  id: test
  name: "OperationsCollisions"
  QtObject {
    id: service
    property string cliPath: "/filetree"
    property var calls: []
    function backendCommand(kind) { return [cliPath, "_backend", kind] }
    function backendRequest(kind, arguments, generation, callback) {
      calls = calls.concat([{ kind: kind, arguments: arguments, callback: callback }])
      return "request-" + calls.length
    }
    function rootName(path) { return String(path).split("/").pop() }
    function closeActionMenu() {}
    property var cancellations: []
    function cancelBackendRequest(id, generation) { cancellations.push([id, generation]) }
  }
  Controllers.OperationController { id: operations; service: service }

  function item(id, kind, parent) {
    return { id: String(id), parent: parent === undefined ? null : String(parent), source: "/source/" + id,
      destination: "/target/" + id, collision: true, incoming_collision: false, same_target: false,
      source_identity: { kind: kind || "file" }, destination_identity: { kind: kind || "file" },
      choices: kind === "directory" ? ["merge", "replace", "keep-both", "skip", "cancel"] : ["replace", "keep-both", "skip", "cancel"] }
  }

  function init() {
    operations.active = null
    operations.activeBackendRequestId = ""
    operations.queue = []
    operations.collisionPlan = null
    operations.collisionItem = null
    operations.collisionDecisions = []
    operations.cancelRequested = false
    operations.busy = false
    service.calls = []
    service.cancellations = []
  }

  function test_archive_and_permission_cancellation_data() {
    return ["archive-create", "archive-extract", "permissions-set"].map(function(command) { return { tag: command, command: command } })
  }

  function test_archive_and_permission_cancellation(data) {
    var id = operations.enqueue("Fixture", service.backendCommand(data.command), false)
    compare(service.calls[0].kind, data.command)
    compare(operations.active.cancellable, true)
    compare(operations.cancel(id), "cancelling")
    compare(service.cancellations, [["request-1", id]])
    compare(operations.cancel(id), "cancelling")
    compare(service.cancellations.length, 1)
  }

  function begin(items) {
    operations.moveSelectionTo("/target", false, ["/source/one"])
    compare(service.calls[0].kind, "transfer-preflight")
    verify(service.calls[0].arguments.indexOf("--journal-id") < 0)
    service.calls[0].callback({ ok: true, decision_id: "decision-one", items: items })
  }

  function test_queue_waits_for_collision_and_same_type_scope_skips_second_prompt() {
    begin([item(0), item(1)])
    compare(service.calls.length, 1)
    compare(operations.collisionItem.id, "0")
    var first = operations.activeId
    operations.moveSelectionTo("/other", true, ["/source/two"])
    compare(operations.activeId, first)
    compare(operations.queue.length, 1)
    verify(operations.resolveCollision(operations.collisionSerial, "keep-both", true))
    tryVerify(function() { return service.calls.length === 2 })
    compare(service.calls[1].kind, "transfer-execute")
    var arguments = service.calls[1].arguments
    var decisions = JSON.parse(arguments[arguments.indexOf("--decisions") + 1])
    compare(decisions.length, 2)
    compare(decisions[1].action, "keep-both")
  }

  function test_stale_dialog_answer_is_ignored_and_cancel_discards_the_plan() {
    begin([item(0)])
    verify(!operations.resolveCollision(operations.collisionSerial - 1, "replace", false))
    compare(service.calls.length, 1)
    verify(operations.resolveCollision(operations.collisionSerial, "cancel", false))
    compare(service.calls[1].kind, "transfer-execute")
    verify(service.calls[1].arguments.indexOf("--cancel") >= 0)
    compare(operations.collisionItem, null)
  }

  function test_expanded_merge_is_another_decision_and_not_completion() {
    begin([item(0, "directory")])
    verify(operations.resolveCollision(operations.collisionSerial, "merge", false))
    tryVerify(function() { return service.calls.length === 2 })
    service.calls[1].callback({ ok: true, requires_decision: true, decision_id: "decision-two",
      accepted_decisions: [{ id: "0", action: "merge" }], items: [item(0, "directory"), item(1, "file", 0)] })
    compare(operations.collisionItem.id, "1")
    verify(operations.busy)
    compare(service.calls.length, 2)
    verify(operations.resolveCollision(operations.collisionSerial, "skip", false))
    tryVerify(function() { return service.calls.length === 3 })
    compare(service.calls[2].arguments[1], "decision-two")
  }

  function test_cancel_between_choice_and_deferred_submit_sends_only_one_request() {
    begin([item(0)])
    verify(operations.resolveCollision(operations.collisionSerial, "replace", false))
    compare(operations.cancelActive(), "cancelling")
    wait(1)
    compare(service.calls.length, 2)
    verify(service.calls[1].arguments.indexOf("--cancel") >= 0)
  }

  function test_cut_clipboard_keeps_skipped_and_unfinished_sources() {
    operations.active = { clearClipboard: true, clearExternal: true }
    operations.clipboardPaths = ["/moved", "/skipped"]
    operations.clipboardMode = "cut"
    operations.externalPaths = ["/moved", "/skipped"]
    operations.externalMode = "cut"
    operations.clearAfterOperation({ completed_sources: ["/moved"] })
    compare(operations.clipboardPaths, ["/skipped"])
    compare(operations.externalPaths, ["/skipped"])
    compare(operations.clipboardMode, "cut")
    operations.clearAfterOperation({ completed_sources: [] })
    compare(operations.clipboardPaths, ["/skipped"])
  }
}
