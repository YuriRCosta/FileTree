import QtQuick
import QtTest
import "../../lib/ImageGallery.js" as ImageGallery

TestCase {
  name: "ImageGallery"

  function item(name, date, extra) {
    var result = { path: "/library/" + name + ".png", name: name, date: date || "", text: name, fields: {} }
    if (extra) for (var key in extra) result[key] = extra[key]
    return result
  }

  function sample() {
    return [
      item("late", "2024-09-10T08:00:00Z"),
      item("early", "2024-09-01T08:00:00Z"),
      item("older", "2023-02-14"),
      item("undated", ""),
      item("newest", "2025-01-03T12:00:00Z")
    ]
  }

  function plan(items, width, cell) {
    return ImageGallery.layout(items, width, { cell: cell || 100, tileHeight: cell || 100, gap: 10, padding: 0, headerHeight: 20, sectionGap: 0 })
  }

  function test_size_steps_are_five_and_clamp() {
    compare(ImageGallery.stepCount(), 5)
    compare(ImageGallery.clampStep(-3), 0)
    compare(ImageGallery.clampStep(9), 4)
    compare(ImageGallery.clampStep("x"), ImageGallery.DEFAULT_STEP)
    verify(ImageGallery.cellFor(0) < ImageGallery.cellFor(4))
    compare(ImageGallery.thumbnailEdge(0), 256)
    compare(ImageGallery.thumbnailEdge(4), 512)
  }

  function test_columns_fit_the_width_with_gaps() {
    compare(ImageGallery.columnsFor(320, 100, 10), 3)
    compare(ImageGallery.columnsFor(429, 100, 10), 3)
    compare(ImageGallery.columnsFor(430, 100, 10), 4)
    compare(ImageGallery.columnsFor(10, 100, 10), 1)
  }

  function test_months_group_newest_first_and_undated_last() {
    var groups = ImageGallery.groups(sample())
    compare(groups.map(function(group) { return group.key }), ["2025-01", "2024-09", "2023-02", ""])
    compare(groups[0].label, "January 2025")
    compare(groups[0].shortLabel, "Jan 2025")
    compare(groups[1].items.map(function(entry) { return entry.name }), ["late", "early"])
    compare(groups[3].label, "Undated")
  }

  function test_layout_rows_carry_positions_and_sections() {
    var result = plan(sample(), 210)
    compare(result.columns, 2)
    compare(result.items.map(function(entry) { return entry.name }), ["newest", "late", "early", "older", "undated"])
    compare(result.rows.map(function(row) { return row.kind }), ["header", "tiles", "header", "tiles", "header", "tiles", "header", "tiles"])
    compare(result.rows[1].y, 20)
    compare(result.rows[2].y, 120)
    compare(result.rows[3].start, 1)
    compare(result.rows[3].count, 2)
    compare(result.sections.length, 4)
    compare(result.sections[1].first, 1)
    compare(result.sections[1].count, 2)
    compare(result.sections[1].y, 120)
    compare(result.height, 4 * 120)
  }

  function test_row_lookup_by_index_and_position() {
    var result = plan(sample(), 210)
    compare(ImageGallery.rowOf(result, 0), 1)
    compare(ImageGallery.rowOf(result, 2), 3)
    compare(ImageGallery.rowOf(result, 9), -1)
    compare(ImageGallery.rowAt(result, 0), 0)
    compare(ImageGallery.rowAt(result, 25), 1)
    compare(ImageGallery.rowAt(result, 125), 2)
    compare(ImageGallery.rowAt(result, 145), 3)
    compare(ImageGallery.rowAt(result, 9999), 7)
    compare(ImageGallery.fractionOf(result, 240), 0.5)
    compare(ImageGallery.positionOf(result, 0.25), 120)
  }

  function test_cursor_moves_across_rows_and_sections() {
    var items = []
    for (var i = 0; i < 7; i++) items.push(item("a" + i, "2024-09-0" + (9 - i)))
    items.push(item("b0", "2024-08-01"))
    var result = plan(items, 320)
    compare(result.columns, 3)
    compare(ImageGallery.moveCursor(result, -1, "down"), 0)
    compare(ImageGallery.moveCursor(result, 0, "right"), 1)
    compare(ImageGallery.moveCursor(result, 1, "down"), 4)
    compare(ImageGallery.moveCursor(result, 4, "down"), 6)
    compare(ImageGallery.moveCursor(result, 6, "down"), 7)
    compare(ImageGallery.moveCursor(result, 7, "up"), 6)
    compare(ImageGallery.moveCursor(result, 5, "up"), 2)
    compare(ImageGallery.moveCursor(result, 2, "up"), 0)
    compare(ImageGallery.moveCursor(result, 3, "left"), 2)
    compare(ImageGallery.moveCursor(result, 0, "left"), 0)
    compare(ImageGallery.moveCursor(result, 7, "right"), 7)
    compare(ImageGallery.moveCursor(result, 4, "home"), 0)
    compare(ImageGallery.moveCursor(result, 4, "end"), 7)
    compare(ImageGallery.moveCursor(plan([], 320), 0, "down"), -1)
  }

  function test_timeline_marks_one_per_year_and_one_per_month() {
    var result = plan(sample(), 210)
    var years = ImageGallery.yearMarks(result.sections, result.height)
    compare(years.map(function(mark) { return mark.label }), ["2025", "2024", "2023"])
    compare(years[1].fraction, 0.25)
    var months = ImageGallery.monthMarks(result.sections, result.height)
    compare(months.length, 4)
    compare(months[3].undated, true)
    compare(ImageGallery.sectionAtFraction(result.sections, result.height, 0.3).key, "2024-09")
    compare(ImageGallery.sectionAtFraction(result.sections, result.height, 0).key, "2025-01")
    compare(ImageGallery.sectionAtFraction(result.sections, result.height, 1).key, "")
    compare(ImageGallery.spacedMarks(years, 100, 30).map(function(mark) { return mark.label }), ["2025", "2023"])
  }

  function test_filter_uses_the_shared_search_grammar() {
    var items = [
      item("bonk-desk", "2024-01-01", { text: "bonk sits at a desk", fields: { character: ["bonk"], tag: ["computer"] } }),
      item("bink-fire", "2024-02-01", { text: "bink puts out a fire", fields: { character: ["bink"], tag: ["danger"] } })
    ]
    compare(ImageGallery.filter(items, "", ["character", "tag"]).items.length, 2)
    compare(ImageGallery.filter(items, "fire", ["character", "tag"]).items.map(function(entry) { return entry.name }), ["bink-fire"])
    compare(ImageGallery.filter(items, "character:bonk", ["character", "tag"]).items.map(function(entry) { return entry.name }), ["bonk-desk"])
    compare(ImageGallery.filter(items, "-tag:danger", ["character", "tag"]).items.map(function(entry) { return entry.name }), ["bonk-desk"])
    compare(ImageGallery.filter(items, "[", ["character", "tag"], { regex: true }).items.length, 0)
    verify(ImageGallery.filter(items, "[", ["character", "tag"], { regex: true }).invalid !== "")
  }

  function test_sort_by_date_keeps_undated_last() {
    var sorted = ImageGallery.sortByDate(sample())
    compare(sorted.map(function(entry) { return entry.name }), ["newest", "late", "early", "older", "undated"])
    compare(ImageGallery.sortByDate(sample(), false).map(function(entry) { return entry.name }), ["older", "early", "late", "newest", "undated"])
  }

  function test_dates_parse_from_iso_and_epoch() {
    compare(ImageGallery.monthKey({ date: "2024-09-10" }), "2024-09")
    compare(ImageGallery.monthKey({ date: 1725955200 }), "2024-09")
    compare(ImageGallery.monthKey({ date: "" }), "")
    compare(ImageGallery.monthKey({ date: "nonsense" }), "")
    compare(ImageGallery.monthLabel("2024-13"), "Undated")
  }
}
