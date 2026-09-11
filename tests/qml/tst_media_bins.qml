import QtQuick
import QtTest
import "../../lib/MediaDates.js" as Dates
import "../../lib/MediaBins.js" as Bins

TestCase {
  name: "MediaBins"
  property var monday: ({ firstDay: 1, minimumDays: 4 })
  property var sunday: ({ firstDay: 7, minimumDays: 1 })

  function items(dates) { return dates.map(function(date, index) { return { path: "/" + index, date: date } }) }
  function sum(bins) { return bins.reduce(function(total, entry) { return total + entry.count }, 0) }
  function find(bins, key) { return bins.filter(function(entry) { return entry.key === key })[0] }

  function test_sparse_months_and_undated() {
    var records = Bins.records(items(["2024-01-01", "2024-03-01", "2024-03", "", "broken"]))
    var result = Bins.build(records, "months", null, monday)
    compare(result.bins.length, 13)
    compare(find(result.bins, "2024-02").count, 0)
    compare(find(result.bins, "2024-03").count, 2)
    compare(result.bins[result.bins.length - 1].key, "undated")
    compare(result.bins[result.bins.length - 1].count, 2)
    compare(sum(result.bins), 5)
    compare(result.maximum, 2)
  }

  function test_unknown_precision_is_not_a_fabricated_day() {
    var records = Bins.records(items(["2024", "2024-02", "2024-02-29", ""]))
    var years = Bins.build(records, "years", null, monday)
    var months = Bins.build(records, "months", years.bins[0], monday)
    compare(sum(months.bins), 3)
    compare(find(months.bins, "unknown").count, 1)
    var month = find(months.bins, "2024-02")
    var weeks = Bins.build(records, "weeks", month, monday)
    compare(sum(weeks.bins), month.count)
    compare(find(weeks.bins, "unknown").count, 1)
    var days = Bins.build(records, "days", month, monday)
    compare(find(days.bins, "2024-02-01").count, 0)
    compare(find(days.bins, "2024-02-29").count, 1)
    compare(find(days.bins, "unknown").count, 1)
  }

  function test_clipped_weeks_conserve_every_day_under_both_rules() {
    var dates = []
    for (var day = Dates.ordinal(2020, 12, 1); day < Dates.ordinal(2022, 1, 1); day++) dates.push(Dates.dayKey(day))
    var records = Bins.records(items(dates))
    ;[monday, sunday].forEach(function(rule) {
      var months = Bins.build(records, "months", null, rule)
      months.bins.filter(function(month) { return month.level === "months" }).forEach(function(month) {
        var weeks = Bins.build(records, "weeks", month, rule)
        compare(sum(weeks.bins), month.count)
        var seen = []
        weeks.bins.forEach(function(week) {
          verify(week.start >= month.start)
          verify(week.end <= month.end)
          verify(week.end - week.start <= 7)
          var days = Bins.build(records, "days", week, rule)
          compare(sum(days.bins), week.count)
          seen = seen.concat(week.indices)
        })
        compare(seen.length, month.indices.length)
        compare(Array.from(new Set(seen)).length, seen.length)
      })
    })
    var january = find(Bins.build(records, "months", null, monday).bins, "2021-01")
    var clipped = Bins.build(records, "weeks", january, monday).bins[0]
    compare(clipped.key, "2020-W53")
    compare(clipped.end - clipped.start, 3)
  }

  function test_empty_and_long_history() {
    var empty = Bins.build([], "months", null, monday)
    compare(empty.count, 0)
    compare(empty.bins[0].key, "undated")
    var result = Bins.build(Bins.records(items(["1900-01", "2024-12"])), "months", null, monday)
    compare(result.level, "years")
    compare(result.bins.length, 126)
    compare(sum(result.bins), 2)
  }

  function test_counts_follow_input_filter_and_timezone() {
    var records = Bins.records(items(["2024-03-01T00:30:00+02:00", "2024-03-01T12:00:00Z"]), 0)
    var result = Bins.build(records.slice(0, 1), "months", null, sunday)
    compare(find(result.bins, "2024-02").count, 1)
    compare(find(result.bins, "2024-03").count, 0)
    compare(sum(result.bins), 1)
  }
}
