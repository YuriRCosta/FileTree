import QtQuick
import QtTest
import "../../ui" as Ui

TestCase {
  id: test
  name: "MediaSlider"
  width: 320
  height: 120
  visible: true
  when: windowShown
  property bool browserKey: false
  Keys.onPressed: function(event) {
    if (event.key === Qt.Key_Left && event.modifiers & Qt.AltModifier) { browserKey = true; event.accepted = true }
  }

  Item { id: wrapper; anchors.fill: parent }

  Component {
    id: factory
    Ui.ImageSizeControl {
      onStepRequested: function(next) { step = next }
    }
  }

  function test_facade_and_clamping() {
    var slider = createTemporaryObject(factory, wrapper)
    compare(slider.step, 2)
    compare(slider.steps, 5)
    compare(slider.shortcutSmaller, "-")
    compare(slider.shortcutLarger, "+")
    slider.request(-10)
    compare(slider.step, 0)
    slider.request(99)
    compare(slider.step, 4)
    slider.request(NaN)
    compare(slider.step, 2)
    slider.request(3.9)
    compare(slider.step, 3)
    slider.step = -5
    compare(slider.clamped, 0)
    slider.step = 10
    compare(slider.clamped, 4)
  }

  function test_keyboard_keeps_focus_and_views_independent() {
    var first = createTemporaryObject(factory, wrapper)
    var second = createTemporaryObject(factory, wrapper, { y: 50 })
    first.takeFocus()
    tryCompare(first, "activeFocus", true)
    keyClick(Qt.Key_Home)
    compare(first.step, 0)
    for (var next = 1; next <= 4; next++) {
      keyClick(Qt.Key_Right)
      compare(first.step, next)
      verify(first.activeFocus)
    }
    keyClick(Qt.Key_Right)
    compare(first.step, 4)
    keyClick(Qt.Key_Minus)
    compare(first.step, 3)
    keyClick(Qt.Key_Down)
    compare(first.step, 2)
    keyClick(Qt.Key_End)
    compare(first.step, 4)
    keyClick(Qt.Key_Home)
    keyClick(Qt.Key_Plus)
    compare(first.step, 1)
    verify(first.activeFocus)
    compare(second.step, 2)
    browserKey = false
    keyClick(Qt.Key_Left, Qt.AltModifier)
    compare(first.step, 1)
    verify(browserKey)
    keyClick(Qt.Key_Right, Qt.ControlModifier)
    compare(first.step, 1)
    keyClick(Qt.Key_Tab)
    verify(second.activeFocus)
    keyClick(Qt.Key_Backtab)
    verify(first.activeFocus)
  }

  function test_drag_all_stops_and_buttons_at_narrow_width() {
    var slider = createTemporaryObject(factory, wrapper, { width: 132 })
    var range = findChild(slider, "densityRange")
    verify(range !== null)
    waitForRendering(slider)
    var start = range.handle.width / 2
    var travel = range.width - range.handle.width
    verify(slider.visible, "visible " + slider.visible)
    verify(range.width > 28 && range.height >= 24, "geometry " + range.width + " " + range.height + " x=" + range.x)
    mousePress(range, start + travel / 2, range.height / 2)
    verify(slider.pressed, "press " + range.value + " " + range.position)
    for (var i = 0; i <= 4; i++) {
      mouseMove(range, start + travel * i / 4, range.height / 2, 30, Qt.LeftButton)
      tryCompare(slider, "step", i)
      verify(slider.pressed)
      verify(slider.activeFocus)
    }
    mouseMove(range, -20, range.height / 2, 30, Qt.LeftButton)
    compare(slider.step, 0)
    verify(slider.pressed)
    mouseRelease(range, -20, range.height / 2)
    verify(!slider.pressed)
    mouseClick(slider, slider.width - 36, slider.height / 2)
    compare(slider.step, 1)
    mouseClick(slider, 12, slider.height / 2)
    compare(slider.step, 0)
    verify(slider.activeFocus)
  }
}
