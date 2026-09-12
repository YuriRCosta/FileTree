import QtQuick
import QtTest
import "../../controllers"
import "../../lib/PathText.js" as PathText

TestCase {
  id: suite
  name: "TailnetNavigation"
  property var requests: []
  property var tree: null
  property var location: null
  property var navigation: null
  Component { id: treeComponent; TreeController {} }
  Component { id: locationComponent; LocationController {} }
  Component { id: navigationComponent; NavigationController {} }
  DrivesController { id: drives; service: files }
  QtObject { id: host; property string focusedMonitorName: "fixture" }
  QtObject {
    id: files
    property var drivesController: drives
    property var bladeHost: host
    property bool backendReady: false
    property bool stateReady: false
    property bool open: true
    property string rootPath: "/local"
    property bool showHidden: false
    property bool gitEnabled: true
    property bool quickNavActive: false
    property string searchQuery: ""
    property var searchModel: ({ clear: function() {} })
    property var favoritesModel: null
    property var recentModel: null
    property string selectedPath: ""
    property var selectedPaths: []
    property var selectedEntries: []
    property string selectionAnchorPath: ""
    property int gitStatusPollIntervalMs: 0
    property string queuedGitMetadataReason: ""
    property bool watcherRunning: false
    property var treeSort: []
    property var treeFilter: ({})
    property var priorityColumns: []
    property bool trashMode: false
    property bool recentMode: false
    property bool drivesMode: false
    property string rootRecoveryOrigin: ""
    property string rootRecoveryNotice: ""
    property string drivesResource: "drives:///"
    property string trashResource: "trash:///"
    property string recentResource: "recent:///"
    property var rootBackStack: []
    property var rootForwardStack: []
    property int searchGitRepositoryCount: 0
    property var treeModel: suite.tree ? suite.tree.model : null
    property string home: "/local"
    function normalizeRoot(path) { return PathText.isRemote(path) ? path : PathText.normalize(path, home) }
    function rootName(path) { return path }
    function setRootPath(path, history, forward) { suite.navigation.setRootPath(path, history, forward) }
    function resetTree() { suite.tree.resetTree() }
    function clearSelection() { selectedPaths = []; selectedPath = "" }
    function scheduleStateSave() {}
    function recordZoxideVisit(path) { verify(path.indexOf("sftp://") !== 0) }
    function focusTree() {}
    function scheduleWatcherRestart() {}
    function treeRowsReplacing() {}
    function requestVisibleGitMetadataRefresh() {}
    function makeRow(raw, depth) { return suite.tree.treeRowSnapshot({ name: raw.name, path: raw.path, isDir: !!raw.is_dir, depth: depth, kind: raw.is_dir ? "Directory" : "File" }) }
    function listingOrderArguments(limit) { return suite.tree.listingOrderArguments(limit) }
    function windowLimitFor(paths) { return suite.tree.windowLimitFor(paths) }
    function backendRequest(command, args, generation, callback) {
      suite.requests.push({ command: command, args: args, callback: callback })
      return "request-" + suite.requests.length
    }
    function cancelBackendRequest() {}
    function backendSubscribeTopic() { return "" }
  }

  function peer() { return { id: "tailnet:peer", kind: "sftp", label: "Peer", canonical_uri: "sftp://user@peer.test/files", connection: "connected", session_generation: "live", capabilities: ["list"] } }
  function inventory() { return { ok: true, locations: [peer()], tailnet: { candidates: [] }, saved: [] } }
  function reply(index, entries, extra) { requests[index].callback(Object.assign({ ok: true, entries: entries, windowed: true }, extra || {})) }
  function init() {
    files.stateReady = false
    files.rootPath = "/local"
    files.rootBackStack = []
    files.rootForwardStack = []
    requests = []
    drives.applyLocations(inventory())
    tree = treeComponent.createObject(suite, { service: files })
    location = locationComponent.createObject(suite, { service: files })
    navigation = navigationComponent.createObject(suite, { service: files })
    verify(tree && location && navigation)
    files.stateReady = true
  }
  function cleanup() {
    files.stateReady = false
    tree.destroy(); location.destroy(); navigation.destroy()
    tree = null; location = null; navigation = null
  }

  function test_descriptor_navigation_expansion_paging_and_stale_response_use_only_list() {
    location.navigateDescriptor(peer(), null, "browse")
    compare(requests[0].command, "list")
    reply(0, [])
    compare(files.rootPath, peer().canonical_uri)
    compare(files.rootBackStack, ["/local"])
    compare(requests[1].command, "list")
    files.gitEnabled = false
    compare(tree.remoteArguments(files.rootPath, 0, 10).filter(function(value) { return value === "--no-git" }).length, 1)
    files.gitEnabled = true
    compare(requests[1].args.slice(0, 6), ["--location", "tailnet:peer", "--generation", "live", "--path", "."])
    var child = peer().canonical_uri + "/space%20%23%25"
    reply(1, [{ path: child, name: "space #%", is_dir: true }], { truncated: true, total: 2 })
    verify(tree.loadMoreChildren(files.rootPath))
    compare(requests[2].command, "list")
    compare(requests[2].args[7], "1")
    reply(2, [{ path: peer().canonical_uri + "/second", name: "second" }], { start: 1, total: 2 })
    tree.toggleDirectory(tree.indexOfTreePath(child))
    compare(requests[3].command, "list")
    compare(requests[3].args[5], "space #%")
    compare(tree.requestVisibleGitMetadataRefresh("manual"), "inactive")
    reply(3, [], { ok: false, error_id: "stale-location", error: "revoked" })
    compare(tree.model.count, 1)
    verify(tree.model.get(0).error.indexOf("reconnect") >= 0)
    compare(drives.descriptorForPath(files.rootPath), null)
    compare(drives.relativePath(peer(), peer().canonical_uri + "/%2e%2e"), null)
    compare(drives.relativePath(peer(), peer().canonical_uri + "/name%2fother"), null)
  }

  function test_history_refreshes_inventory_and_does_not_revive_a_saved_generation() {
    files.rootBackStack = [peer().canonical_uri]
    location.navigate(peer().canonical_uri, null, "back")
    compare(requests[0].command, "locations")
    requests[0].callback(inventory())
    compare(requests[1].command, "list")
    reply(1, [])
    compare(files.rootPath, peer().canonical_uri)
    compare(files.rootBackStack, [])
    compare(files.rootForwardStack, ["/local"])
    compare(requests[2].command, "list")
    reply(2, [])
    navigation.setRootPath("/local")
    compare(requests[3].command, "children-batch")
    compare(files.rootBackStack, [peer().canonical_uri])
    location.navigate(peer().canonical_uri, null, "back")
    compare(requests[4].command, "locations")
    requests[4].callback({ ok: true, locations: [], tailnet: { candidates: [] }, saved: [] })
    compare(files.rootPath, "/local")
    compare(files.rootBackStack, [])
    compare(requests.length, 5)
  }
}
