import QtQuick
import QtTest
import "../../ui"

TestCase {
  name: "ArtifactBinListing"
  property var requests: []
  property var cancelled: []
  property var listing: null

  Item {
    id: first
    function backendRequest(command, arguments, generation, callback) {
      var id = String(requests.length)
      requests.push({id:id, service:first, command:command, arguments:arguments, generation:generation, callback:callback})
      return id
    }
    function cancelBackendRequest(id, generation, discard) { cancelled.push({id:id, service:first, generation:generation, discard:discard}) }
  }
  Item {
    id: second
    function backendRequest(command, arguments, generation, callback) {
      var id = String(requests.length)
      requests.push({id:id, service:second, command:command, arguments:arguments, generation:generation, callback:callback})
      return id
    }
    function cancelBackendRequest(id, generation, discard) { cancelled.push({id:id, service:second, generation:generation, discard:discard}) }
  }
  Component { id: component; ArtifactBinListing { service: first; module: "hooks" } }

  function init() {
    requests = []; cancelled = []
    listing = createTemporaryObject(component, this)
    verify(listing !== null)
  }
  function cleanup() { if (listing) listing.destroy(); listing = null; wait(0) }
  function result(index, name) { requests[index].callback({ok:true, items:[{name:name}]}) }

  function test_refresh_coalesces_without_concurrent_reads() {
    listing.refresh(); listing.refresh(); listing.refresh()
    tryCompare(requests, "length", 1)
    compare(requests[0].command, "bin-list")
    compare(requests[0].arguments, ["--module", "hooks"])
    result(0, "first")
    compare(listing.rows[0].name, "first")
    compare(listing.request, null)
  }

  function test_superseded_read_is_disposed_and_cannot_replace_new_rows() {
    listing.start()
    listing.refresh(); listing.start()
    compare(requests.length, 2)
    compare(cancelled.length, 1)
    verify(cancelled[0].discard)
    result(1, "new"); result(0, "old")
    compare(listing.rows[0].name, "new")
  }

  function test_module_changes_clear_rows_and_capture_the_new_name() {
    listing.start(); result(0, "old")
    listing.refresh(); listing.start()
    listing.module = "skills"
    compare(listing.rows.length, 0)
    listing.start()
    compare(requests[2].arguments, ["--module", "skills"])
    result(1, "old again")
    compare(listing.rows.length, 0)
    result(2, "new")
    compare(listing.rows[0].name, "new")
  }

  function test_service_replacement_cancels_through_the_original_service() {
    listing.start()
    listing.service = second
    compare(cancelled[0].service, first)
    verify(cancelled[0].discard)
    listing.start()
    compare(requests[1].service, second)
    result(0, "old")
    compare(listing.rows.length, 0)
  }

  function test_hidden_view_cancels_and_reopening_refreshes() {
    listing.start()
    listing.active = false
    compare(cancelled.length, 1)
    verify(cancelled[0].discard)
    listing.start(); compare(requests.length, 1)
    result(0, "late")
    compare(listing.rows.length, 0)
    listing.active = true; listing.start()
    compare(requests.length, 2)
  }

  function test_destroyed_owner_discards_the_callback_before_context_deletion() {
    listing.start()
    var request = requests[0]
    listing.destroy(); listing = null
    tryCompare(cancelled, "length", 1)
    compare(cancelled[0].id, request.id)
    compare(cancelled[0].generation, request.generation)
    verify(cancelled[0].discard)
  }

  function test_bad_responses_keep_the_last_good_rows_and_leave_no_active_request() {
    listing.start(); result(0, "first")
    listing.refresh(); listing.start()
    requests[1].callback({ok:false})
    compare(listing.rows.length, 1)
    compare(listing.error, "The bin could not be listed")
    compare(listing.request, null)
  }

  function test_a_failed_listing_keeps_its_rows_and_retries_with_backoff() {
    listing.start(); result(0, "kept")
    listing.refresh()
    tryCompare(requests, "length", 2)
    requests[1].callback({ ok: false, error: "bin store busy" })
    compare(listing.rows[0].name, "kept")
    compare(listing.error, "bin store busy")
    compare(listing.failures, 1)
    tryCompare(requests, "length", 3, 3000)
    compare(requests[2].command, "bin-list")
    requests[2].callback({ ok: true, items: [{ name: "fresh" }] })
    compare(listing.rows[0].name, "fresh")
    compare(listing.error, "")
    compare(listing.failures, 0)
  }

  function test_retries_stop_after_the_limit_and_module_changes_reset_them() {
    listing.failures = listing.maximumRetries
    listing.start()
    requests[0].callback({ ok: false, error: "still busy" })
    compare(listing.failures, listing.maximumRetries)
    wait(1200)
    compare(requests.length, 1)
    listing.module = "skills"
    compare(listing.failures, 0)
    compare(listing.error, "")
  }
}
