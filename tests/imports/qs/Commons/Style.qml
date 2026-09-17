pragma Singleton
import QtQml
QtObject {
  property int cornerRadius: 0
  property var font: ({ family: "monospace", title: 14, body: 12, bodySmall: 11, caption: 10 })
  property var spacing: ({ controlPaddingX: 8, controlPaddingY: 4 })
  function space(value) { return Math.max(0, Math.round(Number(value) || 0)) }
  function hoverFillFor(foreground, accent, urgent) { return Util.alpha(accent, 0.12) }
}
