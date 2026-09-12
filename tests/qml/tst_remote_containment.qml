import QtQuick
import QtTest
import "../../lib/PathText.js" as PathText

TestCase {
  id: suite
  name: "RemoteContainment"
  property int cancelled: 0
  property int cleared: 0
  property int localCalls: 0
  property var testService: service

  QtObject {
    id: service
    property string rootPath: "sftp://user@peer.test/files"
    property string home: "/local"
    property string trashResource: "trash:///"
    property string recentResource: "recent:///"
    property string drivesResource: "drives:///"
    property bool trashMode: false
    property bool recentMode: false
    property bool drivesMode: false
    property var treeModel: ({ count: 0 })
  }

  function source(path) {
    var request = new XMLHttpRequest()
    request.open("GET", Qt.resolvedUrl("../../" + path), false)
    request.send()
    verify(request.responseText.length > 0)
    return request.responseText
  }

  function method(text, name) {
    var start = text.indexOf("  function " + name + "(")
    verify(start >= 0)
    var end = text.indexOf("\n  }", start)
    verify(end >= 0)
    return text.slice(start, end + 4)
  }

  function test_remote_root_reaches_none_of_the_local_helpers() {
    var pane = source("panes/TreePane.qml")
    var media = pane.match(/readonly property bool mediaActive: ([^\n]+)/)[1]
    var methods = method(source("controllers/StateController.qml"), "normalizeRoot")
      + method(source("controllers/WatchController.qml"), "watchedDirectories")
      + method(source("controllers/SearchController.qml"), "runSearch")
      + method(pane, "refreshFolderCount")
    var harness = Qt.createQmlObject('import QtQuick\nimport "../../lib/PathText.js" as PathText\nimport "../../modules/files/ViewChrome.js" as ViewChrome\nQtObject {'
      + 'property var service: suite.testService; property var controller: service; property string rootPath: service.rootPath;'
      + 'property bool mediaMode: true; readonly property bool mediaActive: ' + media + ';'
      + 'property var folderCountData: null; property bool quickNavActive: false; property string quickNavChannel: "files";'
      + 'property bool busy: true; property string error: ""; property string query: "needle";'
      + 'function cancelSearch() { suite.cancelled++ } function clearRows() { suite.cleared++ }'
      + 'function watchPathLimit() { suite.localCalls++; return 0 }'
      + methods + '}', suite, Qt.resolvedUrl("containment-harness.qml"))
    harness.service = service
    compare(harness.normalizeRoot(service.rootPath), service.rootPath)
    compare(PathText.parent(service.rootPath + "/file.txt"), service.rootPath + "/file.txt")
    compare(harness.normalizeRoot("/local/files"), "/local/files")
    compare(harness.watchedDirectories(), [])
    compare(localCalls, 0)
    harness.runSearch()
    compare(cancelled, 1)
    compare(cleared, 1)
    compare(harness.busy, false)
    compare(harness.error, "Remote search is unavailable")
    compare(harness.mediaActive, false)
    harness.refreshFolderCount()
    compare(harness.folderCountData, null)
    harness.destroy()
  }
}
