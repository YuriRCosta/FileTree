.pragma library

var defaults = {
  next: ["j", "Down"], previous: ["k", "Up"],
  first: ["g", "Home"], last: ["G", "End"],
  "page-next": ["Ctrl+D", "PageDown"], "page-previous": ["Ctrl+B", "Ctrl+U", "PageUp"],
  up: ["h", "Left", "Alt+Up"], open: ["l", "Right", "o"], activate: ["Enter"],
  expand: ["z o"], collapse: ["z c"],
  "expand-recursive": ["z O", "Shift+Right"], "collapse-recursive": ["z C", "Shift+Left"],
  "expand-all": ["z R"], "collapse-all": ["z M"],
  quicknav: ["Shift+Z"], picker: ["Ctrl+P"], layout: ["Ctrl+Shift+B"], deep: ["Ctrl+F"],
  search: ["/"], help: ["?"]
}

function chord(value) {
  if (typeof value !== "string" || !value || value.length > 64) throw new Error("Invalid key chord")
  var parts = value.split("+"), name = parts.pop(), modifiers = 0
  var flags = { Ctrl: Qt.ControlModifier, Alt: Qt.AltModifier, Shift: Qt.ShiftModifier, Meta: Qt.MetaModifier }
  for (var part of parts) {
    if (!flags[part] || (modifiers & flags[part])) throw new Error("Invalid modifier in " + value)
    modifiers |= flags[part]
  }
  var names = { Left: Qt.Key_Left, Right: Qt.Key_Right, Up: Qt.Key_Up, Down: Qt.Key_Down,
    Home: Qt.Key_Home, End: Qt.Key_End, PageUp: Qt.Key_PageUp, PageDown: Qt.Key_PageDown,
    Enter: Qt.Key_Return, Return: Qt.Key_Return, Escape: Qt.Key_Escape, Space: Qt.Key_Space,
    Tab: Qt.Key_Tab, Backspace: Qt.Key_Backspace, Delete: Qt.Key_Delete, Insert: Qt.Key_Insert,
    Plus: Qt.Key_Plus, Minus: Qt.Key_Minus, Comma: Qt.Key_Comma, Period: Qt.Key_Period }
  var key = names[name]
  if (key === undefined && /^F([1-9]|[12][0-9]|3[0-5])$/.test(name)) key = Qt.Key_F1 + Number(name.slice(1)) - 1
  if (key === undefined && name.length === 1 && name.charCodeAt(0) >= 33 && name.charCodeAt(0) <= 126) {
    key = name.toUpperCase().charCodeAt(0)
    // Uppercase in a sequence means Shift; Ctrl+B conventionally does not.
    if (parts.length === 0 && /^[A-Z]$/.test(name)) modifiers |= Qt.ShiftModifier
  }
  if (key === undefined) throw new Error("Unknown key: " + name)
  return signature(key, modifiers)
}

function signature(key, modifiers) {
  if (key === Qt.Key_Enter) key = Qt.Key_Return
  if (key === Qt.Key_Backtab) { key = Qt.Key_Tab; modifiers |= Qt.ShiftModifier }
  modifiers &= Qt.ControlModifier | Qt.AltModifier | Qt.ShiftModifier | Qt.MetaModifier
  // Qt already encodes the shifted punctuation in the key itself (? is not /).
  if (key >= 33 && key <= 126 && !(key >= Qt.Key_A && key <= Qt.Key_Z)) modifiers &= ~Qt.ShiftModifier
  return key + ":" + modifiers
}

function prefix(left, right) {
  return left.length <= right.length && left.every(function(key, index) { return key === right[index] })
}

function entriesFor(bindings, problems) {
  var entries = []
  for (var action of Object.keys(bindings)) {
    if (!Object.prototype.hasOwnProperty.call(defaults, action)) {
      if (problems) { problems.push("Unknown action " + action + " is ignored"); continue }
      throw new Error("Unknown action: " + action)
    }
    var values = bindings[action]
    if (!Array.isArray(values) || values.length > 8) {
      if (problems) { problems.push(action + " must be an array of up to 8 bindings; ignored"); continue }
      throw new Error(action + " must be an array of up to 8 bindings")
    }
    for (var value of values) {
      try {
        if (typeof value !== "string" || value.length > 128) throw new Error("Invalid binding for " + action)
        var parts = value.trim().split(/\s+/)
        if (!value.trim() || parts.length > 4) throw new Error("Bindings need 1 to 4 key chords")
        var keys = parts.map(chord)
        if (keys.length > 1 && keys.indexOf(signature(Qt.Key_Escape, 0)) >= 0)
          throw new Error("Escape is reserved for cancelling a key sequence")
        entries.push({ action: action, keys: keys, text: value.trim() })
      } catch (failure) {
        if (!problems) throw failure
        problems.push(String(failure.message || failure) + " (" + action + ")")
      }
    }
  }
  return entries
}

function compile(document) {
  if (!document || typeof document !== "object" || Array.isArray(document)
      || (document.version !== undefined && document.version !== 1)) throw new Error("Expected keybindings version 1")
  var custom = document.bindings === undefined ? ({}) : document.bindings
  if (!custom || typeof custom !== "object" || Array.isArray(custom)) throw new Error("bindings must be an object")
  var problems = []
  var overrides = entriesFor(custom, problems)
  for (var i = 0; i < overrides.length; i++) for (var j = 0; j < i; j++) {
    if (prefix(overrides[i].keys, overrides[j].keys) || prefix(overrides[j].keys, overrides[i].keys))
      throw new Error("Conflicting bindings: " + overrides[j].text + " and " + overrides[i].text)
  }
  var bound = Object.keys(custom).filter(function(action) {
    return Object.prototype.hasOwnProperty.call(defaults, action) && Array.isArray(custom[action]) && custom[action].length <= 8
  })
  var entries = overrides.concat(entriesFor(defaults).filter(function(entry) {
    return bound.indexOf(entry.action) < 0 && !overrides.some(function(other) {
      return prefix(entry.keys, other.keys) || prefix(other.keys, entry.keys)
    })
  }))
  return { entries: entries, custom: bound, problems: problems }
}

function actionFor(action, scope) {
  if (["quicknav", "picker", "layout", "deep"].indexOf(action) >= 0 && scope.indexOf("files") !== 0) return ""
  if (/^(expand|collapse)/.test(action) && scope !== "files-tree" && scope !== "artifacts") return ""
  if (scope === "properties") {
    var properties = { next: "scroll-down", previous: "scroll-up", "page-next": "page-down", "page-previous": "page-up", up: "tree" }
    return properties[action] || action
  }
  return action
}

function legacyBlocked(action, scope) {
  var base = String(action || "").replace(/-extend$/, "")
  return Object.keys(defaults).some(function(key) { return actionFor(key, scope) === base && base !== "" })
}

function label(plan, action) {
  return plan.entries.filter(function(entry) { return entry.action === action }).map(function(entry) { return entry.text }).join(" / ") || "Unbound"
}
