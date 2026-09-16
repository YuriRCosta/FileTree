pragma Singleton
import QtQuick
QtObject {
  function localOrSurfaceSpec(section, token, localColor, defaultColor, fallbackWidth) { return { color: localColor, width: fallbackWidth } }
  function left(spec) { return 0 }
  function right(spec) { return 0 }
  function top(spec) { return 0 }
  function bottom(spec) { return 0 }
}
