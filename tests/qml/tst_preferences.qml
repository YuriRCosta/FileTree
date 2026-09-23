import QtQuick
import QtTest
import "../../controllers"

TestCase {
  name: "ExplicitPreferences"
  property var calls: []
  Item {
    id: host
    property bool layoutReady: false
    property var pendingOpenEdges: null
    property int layoutRevision: 0
    property string configDir: "/nonexistent/filetree-preferences-test"
    property bool opened: false
    property bool rightOpened: false
    function bladeFor(edge) { return { open: edge === "left" ? opened : rightOpened } }
    function setOpen(edge, value) { opened = value }
  }
  Item {
    id: service
    property bool backendReady: false
    property var bladeHost: host
    property var callback: null
    function backendRequest(command, args, generation, fn) {
      calls.push({ command: command, arguments: args })
      callback = fn
      return "request"
    }
    function cancelBackendRequest() {}
  }
  PreferencesController { id: preferences; service: service }
  function init() {
    service.backendReady = false
    preferences.ready = false
    preferences.saving = false
    preferences.settings = ({})
    preferences.pendingTrashDays = 0
    preferences.requestId = ""
    host.layoutReady = false
    host.pendingOpenEdges = null
    host.opened = false
    host.rightOpened = false
    calls = []
  }
  function test_no_authority_until_answer_is_saved() {
    preferences.receive({ok:true, settings:{version:1, trashRetentionDays:null}})
    compare(preferences.pendingTrashDays, 0)
    verify(!preferences.trashAnswered)
    verify(!preferences.trashCleanupConsent)
    preferences.pendingTrashDays = 7
    verify(preferences.setTrashRetentionDays(7, true))
    verify(!preferences.trashCleanupConsent)
    verify(preferences.saving)
    service.callback({ok:true, settings:{version:1, trashRetentionDays:7}})
    verify(preferences.trashAnswered)
    verify(preferences.trashCleanupConsent)
    compare(preferences.trashRetentionDays, 7)
  }
  function test_failure_keeps_pruning_off_and_never_is_a_recorded_answer() {
    preferences.ready = true
    preferences.setTrashRetentionDays(30, true)
    service.callback({ok:false, error:"disk full"})
    verify(!preferences.trashAnswered)
    verify(!preferences.trashCleanupConsent)
    compare(preferences.error, "disk full")
    preferences.setTrashRetentionDays(0, true)
    service.callback({ok:true, settings:{version:1, trashRetentionDays:0}})
    verify(preferences.trashAnswered)
    verify(!preferences.trashCleanupConsent)
  }
  function test_missing_answer_after_a_previous_choice_starts_at_never() {
    preferences.pendingTrashDays = 90
    preferences.receive({ok:true, settings:{version:1, trashRetentionDays:90}})
    verify(preferences.trashCleanupConsent)
    preferences.receive({ok:true, settings:{version:1, trashRetentionDays:null}})
    compare(preferences.pendingTrashDays, 0)
    verify(!preferences.trashAnswered)
    verify(!preferences.trashCleanupConsent)
  }
  function test_unanswered_install_opens_a_blade_after_layout_loads() {
    preferences.receive({ok:true, settings:{version:1, trashRetentionDays:null}})
    preferences.showPendingConsent()
    verify(!host.opened)
    host.layoutReady = true
    tryCompare(host, "opened", true)
  }
  function test_right_only_install_opens_left_for_the_question() {
    host.rightOpened = true
    host.layoutReady = true
    preferences.receive({ok:true, settings:{version:1, trashRetentionDays:null}})
    preferences.showPendingConsent()
    verify(host.opened)
    verify(host.rightOpened)
  }
  function test_question_waits_until_saved_open_state_is_applied() {
    host.pendingOpenEdges = {left:false, right:true}
    host.layoutReady = true
    preferences.receive({ok:true, settings:{version:1, trashRetentionDays:null}})
    preferences.showPendingConsent()
    verify(!host.opened)
    host.pendingOpenEdges = null
    tryCompare(host, "opened", true)
  }
}
