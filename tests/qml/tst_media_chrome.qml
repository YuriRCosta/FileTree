import QtQuick
import QtTest
import "../../modules/files/ViewChrome.js" as Chrome

TestCase {
  name: "MediaChrome"
  ListModel { id: rows }
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
