import QtQuick
import QtTest
import "../../controllers"

TestCase {
  id: suite
  name: "TailnetDrives"
  property var requests: []
  property var cancellations: []

  Item {
    id: service
    property bool backendReady: false
    property bool drivesMode: false
    function backendRequest(command, arguments, generation, callback) {
      suite.requests.push({ command: command, arguments: arguments, generation: generation, callback: callback })
      return String(suite.requests.length)
    }
    function cancelBackendRequest(id, generation) { suite.cancellations.push([id, generation]) }
    function backendSubscribeTopic() { return "" }
    function navigateToLocation() { suite.fail("SFTP must not reach POSIX navigation") }
  }

  DrivesController { id: controller; service: service }
  SignalSpy { id: connections; target: controller; signalName: "connectionRequested" }
  SignalSpy { id: locations; target: controller; signalName: "locationRequested" }

  function descriptor(state, generation) {
    return { id: "tailnet:peer", kind: "sftp", label: "Peer", canonical_uri: "sftp://user@peer.test/files",
      connection: state, session_generation: generation, capabilities: state === "connected" ? ["list"] : [] }
  }

  function inventory(state, generation) {
    return { ok: true, locations: [descriptor(state, generation)], tailnet: { candidates: [{ id: "tailnet:peer", label: "Peer", host: "peer.test" }] },
      saved: [{ host: "peer.test", user: "user", path: "/files" }] }
  }

  function init() {
    service.backendReady = false
    service.drivesMode = false
    controller.applyLocations(null)
    controller.apply({ actions: false, volumes: [] })
    controller.error = ""
    requests = []
    cancellations = []
    connections.clear()
    locations.clear()
    service.backendReady = true
  }

  function test_discovered_peer_stays_listed_and_explicit_connect_uses_saved_fields_without_udisks() {
    compare(requests.length, 0)
    service.drivesMode = true
    compare(requests.length, 1)
    compare(requests[0].command, "locations")
    requests[0].callback(inventory("disconnected", ""))
    compare(controller.volumeCount, 1)
    compare(controller.count, 1)
    compare(controller.model.get(0).source, "tailnet:peer")
    compare(controller.model.get(0).mounted, false)
    compare(controller.model.get(0).sizeLabel, "Not connected")
    compare(controller.actionAvailableFor("tailnet:peer"), true)
    compare(controller.actionAvailableFor("/dev/test"), false)
    controller.openVolume("tailnet:peer", null)
    compare(connections.count, 1)
    compare(Array.from(connections.signalArguments[0]), ["tailnet:peer", "Peer", "peer.test", "user", "/files"])
    compare(requests.length, 1)
    controller.connectPeer("tailnet:peer", "peer.test", "user", "/files", true)
    compare(requests[1].command, "location-connect")
    compare(requests[1].arguments, ["--location", "tailnet:peer", "--expected-host", "peer.test", "--user", "user", "--path", "/files", "--save"])
    controller.connectPeer("tailnet:peer", "peer.test", "user", "/files", true)
    compare(requests.length, 2)
    requests[1].callback({ ok: true, location: descriptor("connected", "new") })
    compare(controller.count, 1)
    compare(requests[2].command, "locations")
    requests[2].callback(inventory("connected", "new"))
    controller.openVolume("tailnet:peer", null)
    compare(locations.count, 1)
    compare(locations.signalArguments[0][0].session_generation, "new")
    controller.apply({ actions: false, volumes: [] })
    compare(controller.count, 1)
    controller.runAction("tailnet:peer")
    compare(requests[3].command, "location-disconnect")
    compare(requests[3].arguments, ["--location", "tailnet:peer", "--generation", "new"])
    controller.openVolume("tailnet:peer", null)
    compare(locations.count, 1)
    requests[3].callback({ ok: true, disconnected: true })
    compare(requests[4].command, "locations")
    requests[4].callback(inventory("disconnected", ""))
    compare(controller.count, 1)
    compare(controller.model.get(0).source, "tailnet:peer")
    compare(controller.model.get(0).mounted, false)
    compare(controller.model.get(0).sizeLabel, "Not connected")
  }

  function test_old_inventory_cannot_replace_action_result_and_cleanup_stays_disconnectable() {
    controller.applyLocations(inventory("disconnected", ""))
    controller.refreshLocations()
    controller.connectPeer("tailnet:peer", "peer.test", "user", "/files", false)
    compare(cancellations.length, 1)
    requests[0].callback(inventory("disconnected", ""))
    var cleanup = descriptor("unavailable", "cleanup")
    requests[1].callback({ ok: false, error: "disconnect required", location: cleanup })
    compare(controller.count, 1)
    requests[2].callback({ ok: true, locations: [cleanup], tailnet: { candidates: [] }, saved: [] })
    compare(controller.error, "disconnect required")
    controller.openVolume("tailnet:peer", null)
    compare(locations.count, 0)
    compare(requests[3].command, "location-disconnect")
    compare(requests[3].arguments, ["--location", "tailnet:peer", "--generation", "cleanup"])
    controller.cancelPeerAction()
    compare(cancellations[1], ["4", requests[3].generation])
    service.backendReady = false
    compare(controller.count, 0)
    requests[3].callback({ ok: true, location: descriptor("connected", "stale") })
    compare(controller.count, 0)
  }
}
