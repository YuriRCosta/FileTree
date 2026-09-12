import QtQuick
import QtTest
import "../../ui" as Ui

TestCase {
  id: test
  name: "MediaView"
  width: 400
  height: 500
  when: windowShown

  QtObject {
    id: fakeController
    property string selectedPath: "/200.png"
    property var selectedPathLookup: ({ "/200.png": true })
    property var dropWheel: ({ dragPaths: [] })
    function selectionUris(paths) { return "" }
    function backendRequest(name, args, generation, callback) { return "test" }
    function cancelBackendRequest(id, generation, discard) {}
  }
  QtObject { id: fakePane; function mediaAllows(action) { return true } }
  Component { id: factory; Ui.MediaView { controller: fakeController; pane: fakePane; width: 380; height: 500 } }

  function test_sort_retains_selection_and_view_anchor() {
    var rows = []
    for (var i = 0; i < 425; i++) rows.push({ path: "/" + i + ".png", name: String(i).padStart(3, "0") + ".png", date: "2024-02-29" })
    var view = createTemporaryObject(factory, test, { items: rows })
    waitForRendering(view)
    view.flickable.forceLayout()
    view.positionViewAtIndex(300, GridView.Beginning)
    view.rememberAnchor()
    var anchor = view.anchorPath
    view.sorts = [{ key: "name", desc: true }]
    waitForRendering(view)
    tryVerify(function() {
      var first = view.flickable.indexAt(1, view.contentY + 1)
      return first <= view.indexOfPath(anchor) && first + view.columns > view.indexOfPath(anchor)
    })
    compare(view.rows[view.currentIndex].path, "/200.png")
    compare(fakeController.selectedPath, "/200.png")
    compare(view.rows[0].path, "/424.png")
  }

  function test_resize_preserves_anchor_and_path() {
    var rows = []
    for (var i = 0; i < 425; i++) rows.push({ path: "/" + i + ".png", name: String(i).padStart(3, "0") + ".png", date: "2024-02-29" })
    var view = createTemporaryObject(factory, test, { items: rows })
    verify(view !== null)
    waitForRendering(view)
    view.flickable.forceLayout()
    view.positionViewAtIndex(200, GridView.Beginning)
    view.rememberAnchor()
    var anchor = view.anchorPath
    verify(anchor !== "")
    compare(view.currentIndex, 200)
    view.sizeStep = 4
    waitForRendering(view)
    tryVerify(function() {
      return view.rows[view.flickable.indexAt(1, view.contentY + 1)].path === anchor
    })
    compare(view.currentIndex, 200)
    view.sizeStep = 0
    waitForRendering(view)
    tryVerify(function() {
      var first = view.flickable.indexAt(1, view.contentY + 1)
      return first <= view.indexOfPath(anchor) && first + view.columns > view.indexOfPath(anchor)
    })
  }
}
