import QtQuick
import QtTest
import "../../controllers"

TestCase {
  name: "UpdateControllerRegression"

  property var requests: []

  Item {
    id: fakeService
    property var manifest: ({ id: "yuricosta.filetree" })
    property double updateCheckedAt: 0
    property bool stateReady: false
    property var replies: ({})
    function markUpdateChecked(timestamp) { updateCheckedAt = timestamp }
    function backendRequest(name, arguments, generation, callback) {
      requests.push({ name: name, arguments: arguments })
      if (replies[name]) callback(replies[name])
    }
  }

  Item {
    id: fakeHost
    property string pluginDir: "/plugins/yuricosta.filetree"
    property var config: ({})
    property var registry: ({
      providerSources: function() {
        return [
          { id: "yuricosta.filetree-memory", dir: "/plugins/yuricosta.filetree-memory" },
          { id: "yuricosta.filetree", dir: "/plugins/yuricosta.filetree" }
        ]
      }
    })
  }

  UpdateController {
    id: controller
    service: fakeService
    host: fakeHost
  }

  function checkReply(rows) {
    return { "update-check": { ok: true, repositories: rows } }
  }

  function init() {
    requests = []
    fakeService.updateCheckedAt = 0
    fakeService.replies = ({})
    fakeHost.config = ({})
    controller.report = ({})
    controller.error = ""
    controller.busy = false
    controller.upToDateNotice = false
    fakeService.stateReady = false
  }

  function test_state_ready_triggers_the_boot_check_once() {
    fakeService.replies = checkReply([])
    fakeService.stateReady = true
    compare(requests.length, 1)
    fakeService.stateReady = false
    fakeService.stateReady = true
    compare(requests.length, 1)
  }

  function test_clean_check_shows_up_to_date_notice_then_clears() {
    fakeService.replies = checkReply([
      { id: "yuricosta.filetree", path: "/p", updatable: false, behind: 0, ahead: 0, dirty: false, backend_stale: false }
    ])
    controller.check()
    verify(controller.upToDateNotice)
    verify(controller.chipVisible)
    compare(controller.chipText, "FileTree is up to date!")
    compare(controller.upToDateNoticeMs, 10000)
    controller.upToDateNotice = false
    verify(!controller.chipVisible)
    compare(controller.chipText, "")
  }

  function test_specs_name_only_the_core_checkout() {
    var specs = controller.repositorySpecs()
    compare(specs, ["yuricosta.filetree=/plugins/yuricosta.filetree"])
    compare(controller.specArguments(specs).slice(0, 2), ["--core", "yuricosta.filetree"])
  }

  function test_stale_check_runs_once_per_interval() {
    fakeService.replies = checkReply([])
    controller.checkIfStale()
    compare(requests.length, 1)
    compare(requests[0].name, "update-check")
    verify(fakeService.updateCheckedAt > 0)
    controller.checkIfStale()
    compare(requests.length, 1)
  }

  function test_disabled_setting_skips_the_check() {
    fakeHost.config = ({ checkUpdates: false })
    controller.checkIfStale()
    compare(requests.length, 0)
  }

  function test_failed_check_reports_and_releases_busy() {
    fakeService.replies = ({ "update-check": { ok: false, error: "remote check failed: offline" } })
    controller.check()
    compare(controller.error, "remote check failed: offline")
    verify(!controller.busy)
    verify(!controller.available)
  }

  function test_report_drives_availability_and_summary() {
    fakeService.replies = checkReply([
      { id: "yuricosta.filetree", path: "/plugins/yuricosta.filetree", updatable: true, behind: 3, ahead: 0, dirty: false, current_version: "0.6.0", upstream_version: "0.7.0", version_change: "newer", backend_stale: false, backend_version: "0.6.0" },
      { id: "yuricosta.filetree-memory", path: "/plugins/yuricosta.filetree-memory", updatable: true, behind: 1, ahead: 0, dirty: false, current_version: "1.0.0", upstream_version: "1.0.0", version_change: "same" },
      { id: "yuricosta.filetree-git", path: "/plugins/yuricosta.filetree-git", updatable: false, behind: 2, ahead: 0, dirty: true, error: "" }
    ])
    controller.check()
    verify(controller.available)
    verify(controller.coreUpdatable)
    compare(controller.chipText, "Update available")
    verify(!controller.upToDateNotice)
    compare(controller.summaryLines(), [
      "Version 0.7.0 of FileTree is now available!"
    ])
    compare(controller.dialogLines().slice(-2), [
      "FileTree only checks for updates; it does not install them while running.",
      "Stop the shell before replacing plugin files; update with omarchy plugin update, then run omarchy restart shell. The backend is included."
    ])
  }

  function test_unknown_version_does_not_guess_or_list_commits() {
    controller.report = { repositories: [
      { id: "yuricosta.filetree", updatable: true, behind: 51, current_version: "0.1.1", upstream_version: "", version_change: "unknown" },
      { id: "yuricosta.filetree-skills", updatable: true, behind: null, upstream_version: "" }
    ] }
    compare(controller.summaryLines(), [
      "An update for FileTree is available; its version could not be determined."
    ])
  }

  function test_companion_rows_never_make_an_update_available() {
    controller.report = { repositories: [
      { id: "yuricosta.filetree", updatable: false },
      { id: "yuricosta.filetree-memory", updatable: true, upstream_version: "0.1.2", version_change: "newer" }
    ] }
    compare(controller.summaryLines(), [])
    verify(!controller.available)
    verify(!controller.coreUpdatable)
  }

  function test_same_or_older_version_is_not_announced_as_a_new_release() {
    controller.report = { repositories: [
      { id: "yuricosta.filetree", updatable: true, current_version: "0.1.2", upstream_version: "0.1.2", version_change: "same" }
    ] }
    compare(controller.summaryLines(), ["FileTree has updates available within version 0.1.2."])
    controller.report = { repositories: [
      { id: "yuricosta.filetree", updatable: true, current_version: "0.1.2", upstream_version: "0.1.1", version_change: "older" }
    ] }
    compare(controller.summaryLines(), ["FileTree's upstream changed to version 0.1.1 (installed: 0.1.2)."])
  }

  function test_stale_backend_shows_reinstall_without_updates() {
    fakeService.replies = checkReply([
      { id: "yuricosta.filetree", path: "/p", updatable: false, behind: 0, ahead: 0, dirty: false, current_version: "0.7.0", upstream_version: "0.7.0", version_change: "newer", backend_stale: true, backend_version: "0.6.0" }
    ])
    controller.check()
    verify(!controller.available)
    verify(controller.backendStale)
    compare(controller.chipText, "Backend update needed")
    compare(controller.summaryLines()[0], "Backend binary is 0.6.0, checkout is 0.7.0: update or reinstall FileTree")
    compare(controller.dialogLines().slice(-1), ["Update or reinstall FileTree, then run omarchy restart shell. Check FILETREE_BINARY if you use a custom backend."])
  }
}
