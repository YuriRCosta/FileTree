import QtQuick
import QtTest
import "../../lib/MediaDates.js" as Dates

TestCase {
  name: "MediaDates"

  function test_local_conversion_and_precision() {
    var stamp = "2024-03-01T00:30:00+02:00"
    var west = Dates.normalize(stamp, "second", -60)
    compare([west.year, west.month, west.day], [2024, 2, 29])
    var east = Dates.normalize(stamp, "second", 180)
    compare([east.year, east.month, east.day], [2024, 3, 1])
    var local = Dates.normalize(stamp)
    var expected = new Date(stamp)
    compare([local.year, local.month, local.day], [expected.getFullYear(), expected.getMonth() + 1, expected.getDate()])
    compare(Dates.normalize("2024-02-29", undefined, -720).day, 29)
    compare(Dates.normalize("2024-02", "second").precision, "month")
    compare(Dates.normalize("2024-02", "second").day, null)
    compare(Dates.normalize({ value: "2024-02-29", precision: "month" }).day, null)
    compare(Dates.normalize("2024-02-29", "unknown").year, null)
    compare(Dates.normalize("2024").month, null)
    compare(Dates.normalize(0, undefined, 0).year, 1970)
    compare(Dates.normalize("2024-02-29T12:30:15.123Z").precision, "millisecond")
  }

  function test_invalid_dates() {
    var inputs = ["", null, undefined, NaN, Infinity, "not a date", "2023-02-29", "1900-02-29", "2024-04-31", "2024-00", "2024-13", "0000-01-01", "2024-02-29garbage", "2024-01-01T24:01:00Z", "2024-01-01T12:60:00Z", "2024-01-01T12:00:00+25:00"]
    inputs.forEach(function(input) { compare(Dates.normalize(input).year, null, String(input)) })
    compare(Dates.normalize("2000-02-29").day, 29)
    compare(Dates.normalize("0099-01-01").year, 99)
  }

  function test_locale_rules_are_independent() {
    compare(Dates.localeRule("en_US", Qt.locale("en_US").firstDayOfWeek), { firstDay: 7, minimumDays: 1, territory: "US" })
    compare(Dates.localeRule("nl_BE", Qt.locale("nl_BE").firstDayOfWeek), { firstDay: 1, minimumDays: 4, territory: "BE" })
    compare(Dates.localeRule("pt_PT", 7).minimumDays, 4)
    compare(Dates.localeRule("zh_Hant_TW", 7).territory, "TW")
    compare(Dates.localeRule("en-AU", 1).minimumDays, 1)
  }

  function test_week_year_data() {
    return [
      { tag: "ISO prior year", day: "2021-01-01", rule: { firstDay: 1, minimumDays: 4 }, year: 2020, week: 53 },
      { tag: "US current year", day: "2021-01-01", rule: { firstDay: 7, minimumDays: 1 }, year: 2021, week: 1 },
      { tag: "ISO next year", day: "2018-12-31", rule: { firstDay: 1, minimumDays: 4 }, year: 2019, week: 1 },
      { tag: "US next year", day: "2023-12-31", rule: { firstDay: 7, minimumDays: 1 }, year: 2024, week: 1 },
      { tag: "Sunday four-day", day: "2021-01-01", rule: { firstDay: 7, minimumDays: 4 }, year: 2020, week: 53 }
    ]
  }

  function test_week_year(data) {
    var result = Dates.week(Dates.normalize(data.day).ordinal, data.rule)
    compare(result.year, data.year)
    compare(result.number, data.week)
    compare(Dates.weekday(result.start), data.rule.firstDay)
  }

  function test_calendar_days_ignore_daylight_saving_lengths() {
    compare(Dates.ordinal(2024, 4, 1) - Dates.ordinal(2024, 3, 30), 2)
    compare(Dates.ordinal(2024, 11, 4) - Dates.ordinal(2024, 11, 2), 2)
    compare(Dates.ordinal(2024, 3, 1) - Dates.ordinal(2024, 2, 28), 2)
  }
}
