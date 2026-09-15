import QtQuick
import QtTest
import "../../lib/FooterFields.js" as FooterFields

TestCase {
  name: "FooterFields"

  function widthOf(text) { return String(text).length }

  function test_defaults_and_normalization_keep_the_chosen_order() {
    compare(FooterFields.normalizeFields().join(","), "count,selected,activity,scope")
    compare(FooterFields.normalizeFields(["scope", "count"]).join(","), "scope,count")
    compare(FooterFields.normalizeFields(["count", "count", "nonsense"]).join(","), "count")
    compare(FooterFields.normalizeFields([]).length, 0)
  }

  function test_the_window_shows_what_fits_and_reports_more() {
    var parts = ["13 items", "1 selected", "folder"]
    var all = FooterFields.window(parts, 0, widthOf, 100, 1)
    compare(all.shown.length, 3)
    compare(all.more, false)
    var tight = FooterFields.window(parts, 0, widthOf, 10, 1)
    compare(tight.shown, ["13 items"])
    compare(tight.more, true)
    compare(tight.back, false)
    var second = FooterFields.window(parts, FooterFields.advance(parts, tight.start, tight.shown.length), widthOf, 10, 1)
    compare(second.shown, ["1 selected"])
    compare(second.more, true)
    compare(second.back, true)
    compare(FooterFields.retreat(parts, second.start, second.shown.length), 0)
  }

  function test_paging_wraps_back_to_the_first_part() {
    var parts = ["a", "b"]
    compare(FooterFields.advance(parts, 0, 1), 1)
    compare(FooterFields.advance(parts, 1, 1), 0)
    compare(FooterFields.window([], 0, widthOf, 50, 1).shown.length, 0)
    compare(FooterFields.window(["only"], 3, widthOf, 1, 1).shown, ["only"])
  }
}
