import QtQuick
import QtTest
import "../../modules/files/ViewChrome.js" as Chrome

TestCase {
  name: "MediaChrome"
  ListModel { id: rows }
  Component {
    id: countClient
    QtObject {
      property var model
      property var count: Chrome.folderCount(model, "/reopen", 1, 1)
    }
  }
  function test_model_count_survives_client_recreation() {
    rows.clear()
    rows.append({ path: "/reopen", depth: 0, expanded: true, loaded: true, loading: false, error: "" })
    rows.append({ depth: 1, kind: "File", gitDeleted: false })
    var first = countClient.createObject(this, { model: rows })
    var value = first.count
    compare(value.loaded, 1)
    first.destroy()
    wait(0)
    gc()
    var second = createTemporaryObject(countClient, this, { model: rows })
    verify(second.count === value)
  }
  function test_unchanged_views_reuse_counts_and_readiness_invalidates_without_revision() {
    var values = [
      { path: "/one", depth: 0, expanded: true, loaded: true, loading: false, error: "" },
      { depth: 1, kind: "File", gitDeleted: false },
      { depth: 1, kind: "More", windowTotal: 1000 }
    ]
    var reads = 0
    var model = { count: values.length, get: function(i) { reads++; return values[i] } }
    compare(Chrome.folderCount(model, "/one", 1, 1), { loaded: 1, total: 1000, known: true })
    reads = 0
    for (var i = 0; i < 20; i++) compare(Chrome.folderCount(model, "/one", 1, 1).total, 1000)
    compare(reads, 20)
    values[1].gitDeleted = true
    compare(Chrome.folderCount(model, "/one", 1, 2).loaded, 0)
    values[0].error = "Refresh failed"
    compare(Chrome.folderCount(model, "/one", 1, 2).known, false)
    values[1].gitDeleted = false
    values[0].error = ""
    compare(Chrome.folderCount(model, "/one", 1, 2).loaded, 1)
    values[0].expanded = false
    compare(Chrome.folderCount(model, "/one", 1, 2).known, false)
    values[0].expanded = true
    values.pop()
    model.count--
    compare(Chrome.folderCount(model, "/one", 1, 2).total, 1)
    compare(Chrome.folderCount(model, "/other", 1, 2).known, false)
    var other = { count: 1, get: function(i) { return values[i] } }
    compare(Chrome.folderCount(other, "/one", 1, 2).total, 0)
  }
  function test_folder_counts_ignore_expanded_descendants_and_paging_sentinel() {
    rows.clear()
    rows.append({ depth: 0, kind: "Directory", gitDeleted: false, windowTotal: 0, expanded: true, loaded: true, loading: false, error: "" })
    rows.append({ depth: 1, kind: "Directory", gitDeleted: false, windowTotal: 0 })
    rows.append({ depth: 2, kind: "File", gitDeleted: false, windowTotal: 0 })
    rows.append({ depth: 2, kind: "More", gitDeleted: false, windowTotal: 10000 })
    rows.append({ depth: 1, kind: "File", gitDeleted: false, windowTotal: 0 })
    rows.append({ depth: 1, kind: "File", gitDeleted: true, windowTotal: 0 })
    rows.append({ depth: 1, kind: "More", gitDeleted: false, windowTotal: 500 })
    compare(Chrome.folderCount(rows), { loaded: 2, total: 500, known: true })
    rows.remove(6)
    compare(Chrome.folderCount(rows), { loaded: 2, total: 2, known: true })
    rows.clear()
    compare(Chrome.folderCount(rows), { loaded: 0, total: 0, known: false })
  }
  function test_collapsed_unloaded_failed_and_empty_scopes() {
    rows.clear()
    rows.append({ path: "/one", depth: 0, kind: "Directory", expanded: false, loaded: false, loading: false, error: "" })
    compare(Chrome.folderCount(rows), { loaded: 0, total: 0, known: false })
    rows.setProperty(0, "expanded", true)
    rows.setProperty(0, "loading", true)
    compare(Chrome.folderCount(rows).known, false)
    rows.setProperty(0, "loaded", true)
    rows.setProperty(0, "loading", false)
    compare(Chrome.folderCount(rows), { loaded: 0, total: 0, known: true })
    compare(Chrome.folderCount(rows, "/one").known, true)
    compare(Chrome.folderCount(rows, "/two").known, false)
    rows.setProperty(0, "error", "Permission denied")
    compare(Chrome.folderCount(rows).known, false)
    rows.setProperty(0, "error", "")
    rows.append({ depth: 1, kind: "More", windowTotal: 427 })
    compare(Chrome.folderCount(rows).total, 427)
    rows.remove(1)
    rows.setProperty(0, "expanded", false)
    rows.setProperty(0, "loaded", false)
    compare(Chrome.folderCount(rows), { loaded: 0, total: 0, known: false })
  }
}
