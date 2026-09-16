.pragma library

var choices = [
  { key: "back", label: "Back", glyph: "" },
  { key: "forward", label: "Forward", glyph: "" },
  { key: "up", label: "Up", glyph: "" },
  { key: "home", label: "Home", glyph: "" },
  { key: "screenshots", label: "Screenshots", glyph: "󰹑" },
  { key: "recent", label: "Recent", glyph: "󰋚" },
  { key: "media", label: "Media", glyph: "󰋩" },
  { key: "drives", label: "Drives", glyph: "󰋊" },
  { key: "desktop-trash", label: "Trash", glyph: "󰩺" }
]

function normalizeFields(value) {
  var keys = choices.map(function(choice) { return choice.key })
  if (value === undefined || value === null) return keys
  var values = Array.isArray(value) ? value : String(value).split(",")
  var wanted = ({})
  for (var i = 0; i < values.length; i++) wanted[String(values[i] || "").trim()] = true
  return keys.filter(function(key) { return wanted[key] === true })
}
