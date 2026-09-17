.pragma library

function escapeHtml(text) {
  return String(text || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
}

function mergeSpans(spans) {
  var sorted = (spans || []).filter(function(span) {
    return Array.isArray(span) && span.length === 2 && span[1] > span[0]
  }).map(function(span) { return [Number(span[0]), Number(span[1])] })
  sorted.sort(function(left, right) { return left[0] - right[0] })
  var merged = []
  for (var i = 0; i < sorted.length; i++) {
    var last = merged.length > 0 ? merged[merged.length - 1] : null
    if (last && sorted[i][0] <= last[1]) last[1] = Math.max(last[1], sorted[i][1])
    else merged.push([sorted[i][0], sorted[i][1]])
  }
  return merged
}

function subsequenceSpans(text, query) {
  var haystack = String(text || "").toLowerCase()
  var spans = []
  var words = String(query || "").toLowerCase().split(/\s+/).filter(function(word) { return word !== "" })
  for (var w = 0; w < words.length; w++) {
    var position = 0
    for (var i = 0; i < words[w].length; i++) {
      var found = haystack.indexOf(words[w][i], position)
      if (found < 0) return spans
      spans.push([found, found + 1])
      position = found + 1
    }
  }
  return mergeSpans(spans)
}

function parseSpans(text) {
  var spans = []
  var parts = String(text || "").split(",")
  for (var i = 0; i < parts.length; i++) {
    var pair = parts[i].split("-")
    if (pair.length !== 2) continue
    var start = Number(pair[0])
    var end = Number(pair[1])
    if (isFinite(start) && isFinite(end) && end > start) spans.push([start, end])
  }
  return mergeSpans(spans)
}

function serializeSpans(spans) {
  return mergeSpans(spans).map(function(span) { return span[0] + "-" + span[1] }).join(",")
}

function markup(text, spans, color) {
  var source = String(text || "")
  var merged = mergeSpans(spans)
  if (merged.length === 0) return escapeHtml(source)
  var result = ""
  var cursor = 0
  for (var i = 0; i < merged.length; i++) {
    var start = Math.max(cursor, Math.min(source.length, merged[i][0]))
    var end = Math.max(start, Math.min(source.length, merged[i][1]))
    result += escapeHtml(source.slice(cursor, start))
    result += "<font color=\"" + String(color) + "\"><b>" + escapeHtml(source.slice(start, end)) + "</b></font>"
    cursor = end
  }
  return result + escapeHtml(source.slice(cursor))
}
