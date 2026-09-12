.pragma library
.import "MediaDates.js" as Dates
.import "TreeOrder.js" as Order

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

function ordered(items, sorts) {
  var ordering = Order.normalizeSorts(sorts)
  var source = records(items)
  source.forEach(function(record) {
    var row = record.item
    record.sortRow = Object.assign({}, row, { git_status: row.gitStatus, git_modified_count: row.gitModifiedCount,
      git_deleted_count: row.gitDeletedCount, git_new_count: row.gitNewCount,
      git_repo_name: row.gitRepoName, git_branch: row.gitBranch, git_worktree: row.gitWorktree })
  })
  return source.sort(function(a, b) {
    var tie = String(a.item.path).localeCompare(String(b.item.path), undefined, { numeric: true }) || a.index - b.index
    if (ordering.length) return Order.compareEntries(a.sortRow, b.sortRow, ordering) || tie
    var left = a.date, right = b.date
    if (left.year === null || right.year === null) return left.year === right.year ? a.index - b.index : (left.year === null ? 1 : -1)
    return left.year - right.year || (left.month === null ? 13 : left.month) - (right.month === null ? 13 : right.month)
      || (left.day === null ? 32 : left.day) - (right.day === null ? 32 : right.day)
      || (left.epoch === null || right.epoch === null ? 0 : left.epoch - right.epoch) || a.index - b.index
  }).map(function(record, index) { return { index: index, item: record.item, date: record.date } })
}

function compact(result, capacity) {
  if (result.bins.length <= capacity || result.level !== "years") return result
  var years = result.bins.filter(function(entry) { return entry.level === "years" })
  var extra = result.bins.filter(function(entry) { return entry.level !== "years" })
  var step = Math.ceil(years.length / Math.max(1, capacity - extra.length)), bins = []
  for (var i = 0; i < years.length; i += step) {
    var part = years.slice(i, i + step), first = part[0], last = part[part.length - 1]
    var entry = bin(first.key + "–" + last.key, "ranges", first.start, last.end)
    part.forEach(function(year) { entry.indices = entry.indices.concat(year.indices); entry.count += year.count })
    entry.indices.sort(function(a, b) { return a - b })
    bins.push(entry)
  }
  bins = bins.concat(extra)
  return { level: "ranges", bins: bins, count: result.count,
    maximum: bins.reduce(function(value, entry) { return Math.max(value, entry.count) }, 0) }
}

function overview(records, capacity, rule) {
  var years = build(records, "years", null, rule)
  var yearCount = years.bins.filter(function(entry) { return entry.level === "years" }).length
  var yearOnly = records.some(function(record) { return record.date.year !== null && record.date.month === null })
  return !yearOnly && yearCount * 12 + 1 <= capacity
    ? build(records, "months", null, rule) : compact(years, capacity)
}

function child(records, parent, capacity, rule) {
  var level = ({ ranges: "years", years: "months", months: "weeks", weeks: "days" })[parent.level]
  return level ? compact(build(records, level, parent, rule), capacity) : null
}

function geometry(bins, columns, pitch, tileHeight) {
  return bins.map(function(entry) {
    var segments = [], row = -2, length = 0
    entry.indices.forEach(function(index) {
      var next = Math.floor(index / columns)
      if (next === row) return
      if (next === row + 1) {
        var previous = segments[segments.length - 1]
        length += next * pitch + tileHeight - previous.bottom
        previous.bottom = next * pitch + tileHeight
      } else {
        segments.push({ top: next * pitch, bottom: next * pitch + tileHeight, offset: length })
        length += tileHeight
      }
      row = next
    })
    return { bin: entry, first: entry.indices.length ? entry.indices[0] : -1,
      last: entry.indices.length ? entry.indices[entry.indices.length - 1] : -1,
      top: segments.length ? segments[0].top : null, bottom: segments.length ? segments[segments.length - 1].bottom : null,
      segments: segments, length: length, columns: columns, pitch: pitch }
  })
}

function after(values, position, valueFor) {
  var low = 0, high = values.length
  while (low < high) {
    var middle = Math.floor((low + high) / 2)
    if (valueFor(values[middle]) <= position) low = middle + 1
    else high = middle
  }
  return low
}

function viewport(bounds, top, height) {
  var start = null, end = null, active = [], first = -1, firstIndex = Infinity
  bounds.forEach(function(bound, index) {
    var segments = bound.segments
    var low = after(segments, top, function(segment) { return segment.bottom })
    var intersects = low < segments.length && segments[low].top < top + height
    active.push(intersects)
    if (!intersects) return
    var rowStart = Math.max(0, Math.floor(top / bound.pitch)) * bound.columns
    var at = after(bound.bin.indices, rowStart - 1, function(value) { return value })
    if (at < bound.bin.indices.length && bound.bin.indices[at] < firstIndex) { firstIndex = bound.bin.indices[at]; first = index }
    for (var i = low; i < segments.length && segments[i].top < top + height; i++) {
      var segment = segments[i]
      var from = index + (segment.offset + Math.max(0, top - segment.top)) / Math.max(1, bound.length)
      var to = index + (segment.offset + Math.min(segment.bottom, top + height) - segment.top) / Math.max(1, bound.length)
      start = start === null ? from : Math.min(start, from)
      end = end === null ? to : Math.max(end, to)
    }
  })
  return { start: start, end: end, active: active, first: first }
}

function seek(bounds, index, fraction, contentHeight, viewportHeight) {
  if (index < 0 || index >= bounds.length || bounds[index].top === null) return null
  var bound = bounds[index]
  var offset = Math.max(0, Math.min(1, fraction)) * bound.length
  var at = after(bound.segments, offset, function(segment) { return segment.offset + segment.bottom - segment.top })
  var segment = bound.segments[Math.min(at, bound.segments.length - 1)]
  return Math.max(0, Math.min(Math.max(0, contentHeight - viewportHeight), segment.top + offset - segment.offset))
}
