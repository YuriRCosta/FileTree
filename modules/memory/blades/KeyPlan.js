.pragma library

function has(modifiers, flag) {
  return !!(modifiers & flag)
}

function action(key, modifiers) {
  var blocked = Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier
  return key === Qt.Key_R && has(modifiers, Qt.ShiftModifier) && !has(modifiers, blocked)
    ? "rescan"
    : ""
}
