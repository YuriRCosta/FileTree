import QtQuick
import QtTest
import "../../modules/files/ViewChrome.js" as Chrome

TestCase {
  name: "MediaChrome"
  ListModel { id: rows }
  function test_folder_counts_ignore_expanded_descendants_and_paging_sentinel() {
    rows.clear()
    rows.append({ depth: 0, kind: "Directory", gitDeleted: false, windowTotal: 0 })
    rows.append({ depth: 1, kind: "Directory", gitDeleted: false, windowTotal: 0 })
    rows.append({ depth: 2, kind: "File", gitDeleted: false, windowTotal: 0 })
    rows.append({ depth: 2, kind: "More", gitDeleted: false, windowTotal: 10000 })
    rows.append({ depth: 1, kind: "File", gitDeleted: false, windowTotal: 0 })
    rows.append({ depth: 1, kind: "File", gitDeleted: true, windowTotal: 0 })
    rows.append({ depth: 1, kind: "More", gitDeleted: false, windowTotal: 500 })
    compare(Chrome.folderCount(rows), { loaded: 2, total: 500 })
    rows.remove(6)
    compare(Chrome.folderCount(rows), { loaded: 2, total: 2 })
    rows.clear()
    compare(Chrome.folderCount(rows), { loaded: 0, total: 0 })
  }
}
