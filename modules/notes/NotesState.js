.pragma library

var FIRST_NOTE_TEXT = "The most merciful thing in the world, I think, is the inability of the human mind to correlate all its contents. We live on a placid island of ignorance in the midst of black seas of infinity, and it was not meant that we should voyage far. The sciences, each straining in its own direction, have hitherto harmed us little; but some day the piecing together of dissociated knowledge will open up such terrifying vistas of reality, and of our frightful position therein, that we shall either go mad from the revelation or flee from the light into the peace and safety of a new dark age. -- HP Lovecraft"

var CAP_BYTES = 65536
var WARN_BYTES = 61440
var MAX_NOTES = 64
var MAX_LABEL_LENGTH = 80

function utf8Length(text) {
  var value = String(text === undefined || text === null ? "" : text)
  var bytes = 0
  for (var index = 0; index < value.length; index++) {
    var code = value.charCodeAt(index)
    if (code < 0x80) bytes += 1
    else if (code < 0x800) bytes += 2
    else if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
      var next = value.charCodeAt(index + 1)
      if (next >= 0xdc00 && next <= 0xdfff) {
        bytes += 4
        index++
        continue
      }
      bytes += 3
    } else bytes += 3
  }
  return bytes
}

function clampToBytes(text, cap) {
  var limit = Number(cap) > 0 ? Number(cap) : CAP_BYTES
  var value = String(text === undefined || text === null ? "" : text)
  if (utf8Length(value) <= limit) return { text: value, clamped: false, bytes: utf8Length(value) }
  var bytes = 0
  var kept = 0
  for (var index = 0; index < value.length; index++) {
    var code = value.charCodeAt(index)
    var width = 1
    var step = 1
    if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
      var next = value.charCodeAt(index + 1)
      if (next >= 0xdc00 && next <= 0xdfff) {
        width = 4
        step = 2
      } else width = 3
    } else if (code >= 0x800) width = 3
    else if (code >= 0x80) width = 2
    if (bytes + width > limit) break
    bytes += width
    kept += step
    index += step - 1
  }
  return { text: value.substring(0, kept), clamped: true, bytes: bytes }
}

function normalize(rawText, rawRevision) {
  var text = ""
  if (typeof rawText === "string") text = rawText
  else if (rawText !== undefined && rawText !== null && typeof rawText === "object" && typeof rawText.text === "string") {
    text = rawText.text
    if (rawText.revision !== undefined) rawRevision = rawText.revision
  }
  var revision = Number(rawRevision)
  if (!isFinite(revision) || revision < 0) revision = 0
  return { text: text, revision: Math.floor(revision), bytes: utf8Length(text) }
}

function hydration(incoming, local) {
  if (local.dirty) return { action: "skip", reason: "local edit pending" }
  if (local.saving) return { action: "skip", reason: "write in flight" }
  if (incoming.revision < local.revision) return { action: "skip", reason: "stale revision" }
  if (incoming.revision === local.revision && incoming.text === local.text) return { action: "skip", reason: "unchanged" }
  return { action: "apply", reason: "" }
}

function persistPlan(text, revision, overCap) {
  if (overCap && utf8Length(text) > CAP_BYTES)
    return { action: "hold", reason: "existing note exceeds the limit", text: text, revision: revision }
  var clamped = clampToBytes(text, CAP_BYTES)
  return {
    action: "write",
    reason: "",
    text: clamped.text,
    clamped: clamped.clamped,
    bytes: clamped.bytes,
    revision: Math.floor(Number(revision) || 0) + 1
  }
}

function statusText(bytes, clamped, overCap, saving, saveFailed) {
  if (saveFailed) return "save rejected by host"
  if (overCap) return "over " + Math.round(CAP_BYTES / 1024) + " KiB, saving paused"
  if (clamped) return "limit reached, trimmed"
  if (saving) return "saving"
  if (bytes >= WARN_BYTES) return Math.round(bytes / 1024) + " KiB of " + Math.round(CAP_BYTES / 1024) + " KiB"
  return bytes + " bytes"
}

function cleanLabel(value, fallback) {
  var label = String(value === undefined || value === null ? "" : value).trim()
  return (label || String(fallback || "Note")).substring(0, MAX_LABEL_LENGTH)
}

function copyNotebook(source) {
  var items = []
  var rows = source && Array.isArray(source.items) ? source.items : []
  for (var index = 0; index < rows.length && index < MAX_NOTES; index++) {
    var row = rows[index] || ({})
    var text = String(row.text || "")
    var item = { id: String(row.id || "note-" + (index + 1)), label: cleanLabel(row.label, "Note " + (index + 1)), text: text,
      cursor: position(row.cursor, text), anchor: position(row.anchor === undefined ? row.cursor : row.anchor, text) }
    var edited = Math.floor(Number(row.edited))
    if (isFinite(edited) && edited > 0) item.edited = edited
    items.push(item)
  }
  var rawNextId = Number(source && source.nextId)
  var nextId = isFinite(rawNextId) && rawNextId >= 1 ? Math.min(1000000000, Math.floor(rawNextId)) : items.length + 1
  return {
    version: 2,
    revision: Math.max(0, Math.floor(Number(source && source.revision) || 0)),
    activeId: String(source && source.activeId || (items.length ? items[0].id : "note-1")),
    nextId: Math.max(items.length + 1, nextId),
    items: items
  }
}

function emptyNotebook(label, text, revision) {
  return {
    version: 2,
    revision: Math.max(0, Math.floor(Number(revision) || 0)),
    activeId: "note-1",
    nextId: 2,
    items: [{ id: "note-1", label: cleanLabel(label, "Note 1"), text: String(text || "") }]
  }
}

function noteIndex(notebook, id) {
  var rows = notebook && Array.isArray(notebook.items) ? notebook.items : []
  var wanted = String(id || "")
  for (var index = 0; index < rows.length; index++) if (String(rows[index].id) === wanted) return index
  return -1
}

function activeIndex(notebook) {
  var index = noteIndex(notebook, notebook ? notebook.activeId : "")
  return index >= 0 ? index : (notebook && notebook.items && notebook.items.length ? 0 : -1)
}

function activeNote(notebook) {
  var index = activeIndex(notebook)
  return index >= 0 ? notebook.items[index] : ({ id: "", label: "Note", text: "" })
}

function textBytes(notebook) {
  var bytes = 0
  var rows = notebook && Array.isArray(notebook.items) ? notebook.items : []
  for (var index = 0; index < rows.length; index++) bytes += utf8Length(rows[index].text)
  return bytes
}

function normalizeNotebook(rawText, rawRevision, legacyLabel) {
  if (!rawText || typeof rawText !== "object" || !Array.isArray(rawText.items)) {
    var legacy = normalize(rawText, rawRevision)
    var migrated = emptyNotebook(legacyLabel, legacy.text, legacy.revision)
    return { notebook: migrated, bytes: legacy.bytes, overCap: legacy.bytes > CAP_BYTES, migrated: true }
  }
  var source = copyNotebook(rawText)
  if (!source.items.length) source = emptyNotebook("Note 1", "", source.revision)
  var taken = ({})
  for (var index = 0; index < source.items.length; index++) {
    var id = String(source.items[index].id || "")
    if (!/^[A-Za-z0-9_.-]{1,64}$/.test(id) || taken[id]) {
      var serial = 1
      id = "note-" + (index + 1)
      while (taken[id]) {
        serial++
        id = "note-" + (index + 1) + "-" + serial
      }
    }
    source.items[index].id = id
    taken[id] = true
  }
  if (noteIndex(source, source.activeId) < 0) source.activeId = source.items[0].id
  var bytes = textBytes(source)
  return { notebook: source, bytes: bytes, overCap: bytes > CAP_BYTES, migrated: false }
}

function notebookHydration(incoming, local) {
  if (local.dirty) return { action: "skip", reason: "local edit pending" }
  if (local.saving) return { action: "skip", reason: "write in flight" }
  if (incoming.notebook.revision < local.revision) return { action: "skip", reason: "stale revision" }
  if (incoming.notebook.revision === local.revision
      && JSON.stringify(incoming.notebook) === JSON.stringify(local.notebook)) return { action: "skip", reason: "unchanged" }
  return { action: "apply", reason: "" }
}

function selectNote(notebook, index) {
  var result = copyNotebook(notebook)
  var wanted = Math.max(0, Math.min(result.items.length - 1, Math.floor(Number(index) || 0)))
  if (result.items.length) result.activeId = result.items[wanted].id
  return result
}

function addNote(notebook) {
  var result = copyNotebook(notebook)
  if (result.items.length >= MAX_NOTES) return null
  var id = "note-" + result.nextId
  result.nextId++
  while (noteIndex(result, id) >= 0) {
    id = "note-" + result.nextId
    result.nextId++
  }
  result.items.push({ id: id, label: "Note " + id.substring(5), text: "" })
  result.activeId = id
  return result
}

function renameNote(notebook, index, label) {
  var result = copyNotebook(notebook)
  var wanted = Math.floor(Number(index))
  if (wanted >= 0 && wanted < result.items.length)
    result.items[wanted].label = cleanLabel(label, "Note " + (wanted + 1))
  return result
}

function removeNote(notebook, index) {
  var result = copyNotebook(notebook)
  var wanted = Math.floor(Number(index))
  if (result.items.length <= 1 || wanted < 0 || wanted >= result.items.length) return result
  var wasActive = result.items[wanted].id === result.activeId
  result.items.splice(wanted, 1)
  if (wasActive) result.activeId = result.items[Math.min(wanted, result.items.length - 1)].id
  return result
}

function editPlan(notebook, value, overCap, now) {
  var result = copyNotebook(notebook)
  var index = activeIndex(result)
  if (index < 0) return { action: "hold", notebook: result, bytes: 0, clamped: false }
  var previous = result.items[index].text
  result.items[index].text = String(value || "")
  var bytes = textBytes(result)
  if (overCap && bytes > CAP_BYTES) {
    stampEdit(result.items[index], previous, now)
    return { action: "hold", notebook: result, bytes: bytes, clamped: false }
  }
  var otherBytes = bytes - utf8Length(result.items[index].text)
  var available = Math.max(0, CAP_BYTES - otherBytes)
  var clamped = available > 0
    ? clampToBytes(result.items[index].text, available)
    : { text: "", clamped: result.items[index].text.length > 0, bytes: 0 }
  result.items[index].text = clamped.text
  stampEdit(result.items[index], previous, now)
  return { action: "write", notebook: result, bytes: otherBytes + clamped.bytes, clamped: clamped.clamped }
}

function stampEdit(item, previous, now) {
  var stamp = Math.floor(Number(now))
  if (item.text !== previous && isFinite(stamp) && stamp > 0) item.edited = stamp
}

function wordCount(text) {
  return (String(text || "").match(/\S+/g) || []).length
}

function characterCount(text) {
  var value = String(text || "")
  return value.length - (value.match(/[\uD800-\uDBFF][\uDC00-\uDFFF]/g) || []).length
}

function tabStop(widths, gap) {
  var minimum = Math.max(1, Math.ceil(Number(gap) || 0))
  var measured = []
  var widest = 0
  for (var index = 0; index < widths.length - 1; index++) {
    var width = Number(widths[index])
    measured.push(isFinite(width) && width > 0 ? width : 0)
    widest = Math.max(widest, measured[index])
  }
  for (var distance = minimum; distance < Math.ceil(widest) + minimum; distance++) {
    var fits = true
    for (var field = 0; field < measured.length && fits; field++) fits = distance - measured[field] % distance >= minimum
    if (fits) return distance
  }
  return Math.ceil(widest) + minimum
}

function persistedNotebook(notebook) {
  var result = copyNotebook(notebook)
  result.revision++
  return result
}

function position(value, text) {
  var index = Math.max(0, Math.min(String(text).length, Math.floor(Number(value) || 0)))
  if (index > 0 && index < text.length && /[\uD800-\uDBFF]/.test(text[index - 1]) && /[\uDC00-\uDFFF]/.test(text[index])) index--
  return index
}

function remember(notebook, cursor, start, end) {
  var note = activeNote(notebook)
  cursor = position(cursor, note.text)
  var anchor = position(cursor === start ? end : start, note.text)
  if (Number(note.cursor || 0) === cursor && Number(note.anchor || 0) === anchor) return notebook
  var next = copyNotebook(notebook)
  next.items[activeIndex(next)].cursor = cursor
  next.items[activeIndex(next)].anchor = anchor
  return next
}

function writtenNotebook(text, slotId) {
  var document
  try { document = JSON.parse(text) } catch (_) { return null }
  for (var edge of ["left", "right"]) {
    var blade = document && document.blades && document.blades[edge]
    for (var slot of blade && Array.isArray(blade.slots) ? blade.slots : []) {
      if (slot.id !== slotId) continue
      for (var entry of slot.modules || []) {
        if (entry.module === "notes") return entry.state && entry.state.text || null
      }
    }
  }
  return null
}
