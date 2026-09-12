import QtQuick
import QtTest
import "../../lib/MediaBins.js" as Bins

TestCase {
  name: "MediaTimelineGeometry"
  property var rule: ({ firstDay: 1, minimumDays: 4 })

  function records(dates) { return Bins.ordered(dates.map(function(date, index) { return { path: "/" + index, date: date } })) }
  function find(result, key) { return result.bins.filter(function(entry) { return entry.key === key })[0] }
  function sum(result) { return result.bins.reduce(function(count, entry) { return count + entry.count }, 0) }

  function test_order_preserves_paths_and_unknown_precision() {
    var source = records(["", "2024", "2023-03", "2023-03-01", "2023-03-01", "2024-01-01"])
    compare(source.map(function(record) { return record.item.path }), ["/3", "/4", "/2", "/5", "/1", "/0"])
    var overview = Bins.overview(source, 100, rule)
    compare(overview.level, "years")
    var months = Bins.child(source, find(overview, "2023"), 40, rule)
    var weeks = Bins.child(source, find(months, "2023-03"), 40, rule)
    compare(sum(weeks), 3)
    compare(find(weeks, "unknown").indices, [2])
    compare(source[2].date.day, null)
  }

  function test_partial_intersections_come_from_tile_rows() {
    var source = records(["2024-01-01", "2024-01-02", "2024-01-03", "2024-03-01", "2024-03-02", "2024-03-03", ""])
    var result = Bins.overview(source, 40, rule)
    var bounds = Bins.geometry(result.bins, 2, 100, 96)
    compare(bounds[0].top, 0)
    compare(bounds[0].bottom, 196)
    compare(bounds[2].top, 100)
    compare(bounds[2].bottom, 296)
    var viewport = Bins.viewport(bounds, 125, 25)
    compare(viewport.active[0], true)
    compare(viewport.active[1], false)
    compare(viewport.active[2], true)
    verify(Math.abs(viewport.start - 125 / 196) < 0.00001)
    verify(Math.abs(viewport.end - (2 + 50 / 196)) < 0.00001)
    compare(Bins.viewport(bounds, 196, 100).active[0], false)
    compare(Bins.seek(bounds, 1, 0, 396, 100), null)
    compare(Bins.seek(bounds, 2, 0, 396, 100), 100)
    compare(Bins.seek(bounds, 12, 1, 396, 100), 296)
    compare(result.maximum, 3)
  }

  function test_long_history_stays_readable_and_reconciles() {
    var source = records(["0001-01-01", "2024-02-29", "9999-12-31", ""])
    var overview = Bins.overview(source, 20, rule)
    verify(overview.bins.length <= 20)
    compare(overview.level, "ranges")
    compare(sum(overview), 4)
    compare(overview.bins[overview.bins.length - 1].key, "undated")
    var scope = overview.bins[0]
    var depth = 0
    while (scope.level === "ranges") {
      var child = Bins.child(source, scope, 20, rule)
      verify(child.bins.length <= 20)
      compare(sum(child), scope.count)
      verify(child.bins[0].end - child.bins[0].start < scope.end - scope.start)
      scope = child.bins[0]
      verify(++depth < 10)
    }
    compare(scope.level, "years")
    compare(Bins.child(source, scope, 20, rule).level, "months")
  }

  function test_explicit_sort_and_disjoint_date_intervals() {
    var rows = [
      { path: "/c", name: "c", size: 1, modified: "2024-01-01", date: "2024-01-01" },
      { path: "/b", name: "b", size: 3, modified: "2024-03-01", date: "2024-03-01" },
      { path: "/a", name: "a", size: 2, modified: "2024-01-01", date: "2024-01-01" }
    ]
    var source = Bins.ordered(rows, [{ key: "name", desc: false }])
    compare(source.map(function(record) { return record.item.path }), ["/a", "/b", "/c"])
    var bounds = Bins.geometry(Bins.overview(source, 20, rule).bins, 1, 100, 96)
    compare(bounds[0].segments.length, 2)
    var middle = Bins.viewport(bounds, 110, 50)
    compare(middle.active[0], false)
    compare(middle.active[2], true)
    compare(middle.first, 2)
    compare(Bins.seek(bounds, 0, 0.5, 296, 20), 200)
    compare(Bins.ordered(rows, [{ key: "size", desc: true }]).map(function(record) { return record.item.path }), ["/b", "/a", "/c"])
    compare(Bins.ordered(rows, [{ key: "modified", desc: true }, { key: "name", desc: true }]).map(function(record) { return record.item.path }), ["/b", "/c", "/a"])
    compare(Bins.ordered(rows.slice().reverse(), [{ key: "modified", desc: true }]).map(function(record) { return record.item.path }), ["/b", "/a", "/c"])
  }

  function test_capacity_and_filtered_counts() {
    var source = records(["2024-02-29", "2026-01-01", "2026-01-02", ""])
    compare(Bins.overview(source, 40, rule).level, "months")
    compare(Bins.overview(source, 20, rule).level, "years")
    var filtered = Bins.overview(records(["2026-01-02"]), 20, rule)
    compare(filtered.maximum, 1)
    compare(sum(filtered), 1)
    var empty = Bins.overview([], 20, rule)
    compare(sum(empty), 0)
    compare(Bins.viewport(Bins.geometry(empty.bins, 1, 100, 96), 0, 400).start, null)
  }
}
