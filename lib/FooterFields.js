.pragma library

var choices = [
  { key: "count", label: "Item count", glyph: "󰆽" },
  { key: "selected", label: "Selection count", glyph: "󰒆" },
  { key: "scope", label: "Scope word", glyph: "󰉋" },
  { key: "activity", label: "Loading and partial state", glyph: "󰔟" },
  { key: "loaded", label: "Loaded of total", glyph: "󰦖" }
]

function normalizeFields(value) {
  if (value === undefined || value === null) return ["count", "selected", "activity", "scope"]
  var values = Array.isArray(value) ? value : String(value).split(",")
  var keys = choices.map(function(choice) { return choice.key })
  var result = []
  for (var i = 0; i < values.length; i++) {
    var key = String(values[i] || "").trim()
    if (keys.indexOf(key) >= 0 && result.indexOf(key) < 0) result.push(key)
  }
  return result
}

function window(parts, offset, widthOf, budget, gap) {
  if (!parts.length) return { shown: [], start: 0, more: false }
  var start = Math.max(0, Math.min(Math.floor(offset) || 0, parts.length - 1))
  var shown = []
  var used = 0
  for (var i = start; i < parts.length; i++) {
    var width = widthOf(parts[i]) + (shown.length ? gap : 0)
    if (shown.length && used + width > budget) break
    shown.push(parts[i])
    used += width
  }
  return { shown: shown, start: start, more: start > 0 || start + shown.length < parts.length }
}

function advance(parts, start, shownCount) {
  var next = start + Math.max(1, shownCount)
  return next >= parts.length ? 0 : next
}
