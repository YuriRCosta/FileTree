import QtQuick
import QtTest
import "../../lib/ToolbarFields.js" as ToolbarFields

TestCase {
  name: "ToolbarFields"

  readonly property string canonical: "back,forward,up,home,screenshots,recent,media,drives,desktop-trash"

  function test_every_button_is_shown_until_a_choice_is_stored() {
    compare(ToolbarFields.normalizeFields().join(","), canonical)
    compare(ToolbarFields.normalizeFields(null).join(","), canonical)
    compare(ToolbarFields.choices.length, 9)
    for (var i = 0; i < ToolbarFields.choices.length; i++) {
      var choice = ToolbarFields.choices[i]
      verify(String(choice.key) !== "")
      verify(String(choice.label) !== "")
      verify(String(choice.glyph) !== "")
    }
  }

  function test_a_stored_choice_keeps_the_canonical_order() {
    compare(ToolbarFields.normalizeFields(["drives", "back"]).join(","), "back,drives")
    compare(ToolbarFields.normalizeFields("up,back,up").join(","), "back,up")
    compare(ToolbarFields.normalizeFields([" home ", "nonsense"]).join(","), "home")
    compare(ToolbarFields.normalizeFields(["desktop-trash", "media"]).join(","), "media,desktop-trash")
  }

  function test_an_empty_toolbar_is_a_legal_stored_choice() {
    compare(ToolbarFields.normalizeFields([]).length, 0)
    compare(ToolbarFields.normalizeFields("").length, 0)
  }

  function test_a_damaged_stored_choice_never_throws() {
    compare(ToolbarFields.normalizeFields(42).length, 0)
    compare(ToolbarFields.normalizeFields({}).length, 0)
    compare(ToolbarFields.normalizeFields(["toString", "constructor"]).length, 0)
    compare(ToolbarFields.normalizeFields([null, undefined, "up"]).join(","), "up")
  }
}
