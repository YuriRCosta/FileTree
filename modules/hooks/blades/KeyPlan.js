.pragma library

var TAB_CYCLE_KEYS = ["Key_Tab", "Key_Backtab", "Key_PageUp", "Key_PageDown",
                      "Key_BracketLeft", "Key_BracketRight"]

function isTabCycle(key, modifiers, qt) {
  if (!(modifiers & qt.ControlModifier)) return false
  for (var i = 0; i < TAB_CYCLE_KEYS.length; i++)
    if (key === qt[TAB_CYCLE_KEYS[i]]) return true
  return false
}

function moduleAction(key, modifiers, qt) {
  if (isTabCycle(key, modifiers, qt)) return ""
  var exclusive = qt.ControlModifier | qt.AltModifier | qt.MetaModifier
  if (key === qt.Key_R && (modifiers & qt.ShiftModifier) && !(modifiers & exclusive)) return "rescan"
  if (key === qt.Key_N && !(modifiers & (exclusive | qt.ShiftModifier))) return "name"
  return ""
}
