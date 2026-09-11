.pragma library
.import "MediaDates.js" as Dates

function records(items, offsetMinutes) {
  return items.map(function(item, index) {
    return { index: index, item: item, date: Dates.normalize(item.date, item.datePrecision, offsetMinutes) }
  })
}

function bin(key, level, start, end) {
  return { key: key, level: level, start: start, end: end, count: 0, indices: [] }
}

function include(target, record) { target.indices.push(record.index); target.count++ }

function build(records, level, scope, rule) {
  var membership = Object.create(null)
  if (scope) scope.indices.forEach(function(index) { membership[index] = true })
  var selected = scope ? records.filter(function(record) { return membership[record.index] === true }) : records
  var dated = selected.filter(function(record) { return record.date.year !== null })
  var bins = [], unknown = bin("unknown", "unknown", null, null), undated = bin("undated", "undated", null, null)
  var start = scope ? scope.start : null, end = scope ? scope.end : null
  if (!scope && dated.length) {
    var minimum = dated[0].date.year, maximumYear = minimum
    dated.forEach(function(record) { minimum = Math.min(minimum, record.date.year); maximumYear = Math.max(maximumYear, record.date.year) })
    start = Dates.ordinal(minimum, 1, 1)
    end = Dates.ordinal(maximumYear + 1, 1, 1)
  }
  if (start !== null && end !== null) {
    if (level === "months" && end - start > 366 * 10) level = "years"
    if ((level === "weeks" || level === "days") && (!scope || end - start > 31)) throw new Error("Fine bins require a clipped month or week scope")
    var cursor = start
    while (cursor < end) {
      var date = Dates.calendar(cursor), next, entry
      if (level === "years") {
        next = Math.min(end, Dates.ordinal(date.year + 1, 1, 1))
        entry = bin(String(date.year), "years", cursor, next)
        entry.year = date.year
      } else if (level === "months") {
        next = Math.min(end, Dates.ordinal(date.year, date.month + 1, 1))
        entry = bin(Dates.monthKey(date.year, date.month), "months", cursor, next)
        entry.year = date.year
        entry.month = date.month
      } else if (level === "weeks") {
        var week = Dates.week(cursor, rule)
        next = Math.min(end, week.start + 7)
        entry = bin(week.year + "-W" + Dates.pad(week.number), "weeks", cursor, next)
        entry.weekYear = week.year
        entry.week = week.number
      } else if (level === "days") {
        next = cursor + 1
        entry = bin(Dates.dayKey(cursor), "days", cursor, next)
      } else throw new Error("Unknown media detail level")
      bins.push(entry)
      cursor = next
    }
  }
  selected.forEach(function(record) {
    var date = record.date
    if (date.year === null) { include(undated, record); return }
    if ((level !== "years" && date.month === null) || ((level === "weeks" || level === "days") && date.day === null)) {
      include(unknown, record)
      return
    }
    var position = date.ordinal === null ? Dates.ordinal(date.year, date.month === null ? 1 : date.month, 1) : date.ordinal
    var low = 0, high = bins.length - 1
    while (low <= high) {
      var middle = Math.floor((low + high) / 2), entry = bins[middle]
      if (position < entry.start) high = middle - 1
      else if (position >= entry.end) low = middle + 1
      else { include(entry, record); return }
    }
    include(unknown, record)
  })
  if (unknown.count) bins.push(unknown)
  if (!scope || undated.count) bins.push(undated)
  var maximum = bins.reduce(function(value, entry) { return Math.max(value, entry.count) }, 0)
  return { level: level, bins: bins, count: selected.length, maximum: maximum }
}
