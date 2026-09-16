.pragma library
.import "SearchQuery.js" as SearchQuery

var SIZE_CELLS = [56, 84, 120, 168, 240]
var DEFAULT_STEP = 2
var MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]
var SHORT_MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sept", "Oct", "Nov", "Dec"]
var UNDATED_LABEL = "Undated"

function clampStep(step) {
  var value = Math.floor(Number(step))
  if (!isFinite(value)) value = DEFAULT_STEP
  return Math.max(0, Math.min(SIZE_CELLS.length - 1, value))
}

function stepCount() {
  return SIZE_CELLS.length
}

function cellFor(step) {
  return SIZE_CELLS[clampStep(step)]
}

function thumbnailEdge(step) {
  return clampStep(step) >= 3 ? 512 : 256
}

function columnsFor(width, cell, gap) {
  var space = Math.max(0, Number(width) || 0)
  var size = Math.max(1, Number(cell) || 1)
  var spacing = Math.max(0, Number(gap) || 0)
  return Math.max(1, Math.floor((space + spacing) / (size + spacing)))
}

function parseDate(value) {
  if (value === undefined || value === null || value === "") return null
  if (typeof value === "number" && isFinite(value)) {
    var stamp = new Date(value < 1e11 ? value * 1000 : value)
    return isNaN(stamp.getTime()) ? null : { year: stamp.getFullYear(), month: stamp.getMonth() + 1 }
  }
  var text = String(value)
  var match = /^(\d{4})-(\d{2})/.exec(text)
  if (match) {
    var month = Number(match[2])
    if (month >= 1 && month <= 12) return { year: Number(match[1]), month: month }
  }
  var parsed = new Date(text)
  return isNaN(parsed.getTime()) ? null : { year: parsed.getFullYear(), month: parsed.getMonth() + 1 }
}

function monthKey(item) {
  var date = parseDate(item ? item.date : null)
  return date ? date.year + "-" + (date.month < 10 ? "0" : "") + date.month : ""
}

function monthLabel(key, short) {
  var match = /^(\d{4})-(\d{2})$/.exec(String(key || ""))
  var month = match ? Number(match[2]) : 0
  if (month < 1 || month > 12) return UNDATED_LABEL
  var names = short ? SHORT_MONTHS : MONTHS
  return names[month - 1] + " " + match[1]
}

function groups(items) {
  var list = Array.isArray(items) ? items : []
  var byKey = {}
  var order = []
  for (var i = 0; i < list.length; i++) {
    var key = monthKey(list[i])
    var group = byKey[key]
    if (!group) {
      var date = parseDate(list[i] ? list[i].date : null)
      group = byKey[key] = { key: key, label: monthLabel(key, false), shortLabel: monthLabel(key, true),
        year: date ? date.year : 0, month: date ? date.month : 0, items: [] }
      order.push(group)
    }
    group.items.push(list[i])
  }
  order.sort(function(left, right) {
    if (left.key === "" || right.key === "") return left.key === "" ? 1 : -1
    return right.key < left.key ? -1 : right.key > left.key ? 1 : 0
  })
  return order
}

function layout(items, width, options) {
  var settings = options && typeof options === "object" ? options : {}
  var cell = Math.max(1, Number(settings.cell) || cellFor(DEFAULT_STEP))
  var tileHeight = Math.max(cell, Number(settings.tileHeight) || cell)
  var gap = Math.max(0, Number(settings.gap) || 0)
  var padding = Math.max(0, Number(settings.padding) || 0)
  var headerHeight = Math.max(0, Number(settings.headerHeight) || 0)
  var sectionGap = Math.max(0, Number(settings.sectionGap) || 0)
  var columns = columnsFor(width - padding * 2, cell, gap)
  var available = Math.max(0, (Number(width) || 0) - padding * 2)
  var label = Math.max(0, tileHeight - cell)
  var stretched = Math.floor((available - gap * (columns - 1)) / columns)
  if (stretched > cell) {
    cell = stretched
    tileHeight = cell + label
  }
  var ordered = []
  var rows = []
  var sections = []
  var y = padding
  var grouped = groups(items)
  for (var g = 0; g < grouped.length; g++) {
    var group = grouped[g]
    var top = y
    var first = ordered.length
    if (headerHeight > 0) {
      rows.push({ kind: "header", label: group.label, key: group.key, section: g, y: y, height: headerHeight, start: first, count: 0 })
      y += headerHeight
    }
    for (var offset = 0; offset < group.items.length; offset += columns) {
      var count = Math.min(columns, group.items.length - offset)
      rows.push({ kind: "tiles", key: group.key, section: g, y: y, height: tileHeight, start: first + offset, count: count })
      y += tileHeight + gap
    }
    for (var i = 0; i < group.items.length; i++) ordered.push(group.items[i])
    if (group.items.length > 0) y -= gap
    sections.push({ key: group.key, label: group.label, shortLabel: group.shortLabel, year: group.year, month: group.month,
      first: first, count: group.items.length, y: top, height: y - top })
    y += sectionGap
  }
  if (grouped.length > 0) y -= sectionGap
  return { rows: rows, sections: sections, items: ordered, height: Math.max(0, y + padding), cell: cell, tileHeight: tileHeight,
    gap: gap, padding: padding, columns: columns, headerHeight: headerHeight }
}

function rowOf(plan, index) {
  var rows = plan && Array.isArray(plan.rows) ? plan.rows : []
  for (var i = 0; i < rows.length; i++) {
    var row = rows[i]
    if (row.kind === "tiles" && index >= row.start && index < row.start + row.count) return i
  }
  return -1
}

function rowAt(plan, y) {
  var rows = plan && Array.isArray(plan.rows) ? plan.rows : []
  if (rows.length === 0) return -1
  var position = Number(y) || 0
  var low = 0
  var high = rows.length - 1
  while (low < high) {
    var middle = Math.floor((low + high + 1) / 2)
    if (rows[middle].y <= position) low = middle
    else high = middle - 1
  }
  return low
}

function fractionOf(plan, y) {
  var height = plan ? Number(plan.height) || 0 : 0
  if (height <= 0) return 0
  return Math.max(0, Math.min(1, (Number(y) || 0) / height))
}

function positionOf(plan, fraction) {
  var height = plan ? Number(plan.height) || 0 : 0
  return Math.max(0, Math.min(1, Number(fraction) || 0)) * height
}

function neighbourRow(plan, rowIndex, delta) {
  var rows = plan.rows
  var index = rowIndex + delta
  while (index >= 0 && index < rows.length) {
    if (rows[index].kind === "tiles") return index
    index += delta
  }
  return -1
}

function moveCursor(plan, index, direction) {
  var total = plan && Array.isArray(plan.items) ? plan.items.length : 0
  if (total === 0) return -1
  var current = Math.max(0, Math.min(total - 1, Math.floor(Number(index))))
  if (!isFinite(current) || index < 0) return direction === "end" ? total - 1 : 0
  if (direction === "home") return 0
  if (direction === "end") return total - 1
  if (direction === "left") return Math.max(0, current - 1)
  if (direction === "right") return Math.min(total - 1, current + 1)
  var rowIndex = rowOf(plan, current)
  if (rowIndex < 0) return current
  var row = plan.rows[rowIndex]
  var column = current - row.start
  var target = neighbourRow(plan, rowIndex, direction === "up" ? -1 : 1)
  if (target < 0) return direction === "up" ? row.start : row.start + row.count - 1
  var next = plan.rows[target]
  return next.start + Math.min(column, next.count - 1)
}

function yearMarks(sections, contentHeight) {
  var list = Array.isArray(sections) ? sections : []
  var height = Number(contentHeight) || 0
  var marks = []
  var seen = {}
  for (var i = 0; i < list.length; i++) {
    var year = Number(list[i].year) || 0
    if (year === 0 || seen[year]) continue
    seen[year] = true
    marks.push({ label: String(year), fraction: height > 0 ? list[i].y / height : 0, section: i })
  }
  return marks
}

function monthMarks(sections, contentHeight) {
  var list = Array.isArray(sections) ? sections : []
  var height = Number(contentHeight) || 0
  var marks = []
  for (var i = 0; i < list.length; i++)
    marks.push({ label: list[i].shortLabel, fraction: height > 0 ? list[i].y / height : 0, section: i, undated: list[i].key === "" })
  return marks
}

function sectionAtFraction(sections, contentHeight, fraction) {
  var list = Array.isArray(sections) ? sections : []
  if (list.length === 0) return null
  var y = Math.max(0, Math.min(1, Number(fraction) || 0)) * (Number(contentHeight) || 0)
  var found = list[0]
  for (var i = 0; i < list.length; i++) {
    if (list[i].y <= y) found = list[i]
    else break
  }
  return found
}

function spacedMarks(marks, trackHeight, minimumSpacing) {
  var list = Array.isArray(marks) ? marks : []
  var height = Number(trackHeight) || 0
  var spacing = Math.max(0, Number(minimumSpacing) || 0)
  var kept = []
  var lastY = -Infinity
  for (var i = 0; i < list.length; i++) {
    var y = list[i].fraction * height
    if (y - lastY < spacing) continue
    kept.push(list[i])
    lastY = y
  }
  return kept
}

function filter(items, query, filterKeys, options) {
  var list = Array.isArray(items) ? items : []
  var spec = SearchQuery.parse(query, filterKeys, options)
  if (spec.invalid) return { items: [], spec: spec, invalid: spec.invalid }
  if (spec.empty) return { items: list, spec: spec, invalid: "" }
  var kept = []
  for (var i = 0; i < list.length; i++)
    if (SearchQuery.matches(spec, list[i])) kept.push(list[i])
  return { items: kept, spec: spec, invalid: "" }
}

function sortByDate(items, descending) {
  var list = Array.isArray(items) ? items.slice() : []
  var direction = descending === false ? 1 : -1
  list.sort(function(left, right) {
    var a = String(left && left.date || "")
    var b = String(right && right.date || "")
    if (a === b) return 0
    if (a === "") return 1
    if (b === "") return -1
    return a < b ? -direction : direction
  })
  return list
}
