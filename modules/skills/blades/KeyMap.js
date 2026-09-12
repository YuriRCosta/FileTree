.pragma library

function hostTabChord(key, modifiers, qt) {
  if (!(modifiers & qt.ControlModifier)) return false
  return key === qt.Key_Tab || key === qt.Key_Backtab
    || key === qt.Key_PageUp || key === qt.Key_PageDown
    || key === qt.Key_BracketLeft || key === qt.Key_BracketRight
}

function resolve(key, modifiers, qt) {
  if (hostTabChord(key, modifiers, qt)) return ""
  var exclusive = qt.ControlModifier | qt.AltModifier | qt.MetaModifier
  if (key === qt.Key_R && (modifiers & qt.ShiftModifier) && !(modifiers & exclusive)) return "rescan"
  return ""
}
