import QtQuick
import QtTest
import "../../ui" as Ui
import "../../lib/KeyBindings.js" as KeyBindings

TestCase {
  name: "TreeKeys"
  Ui.TreeKeys { id: folds; active: true }

  function press(key, modifiers, repeated) {
    return folds.action({ key: key, modifiers: modifiers || Qt.NoModifier }, !!repeated)
  }
  function init() { folds.active = true; folds.scope = "files-tree"; folds.plan = KeyBindings.compile({}); folds.reset() }

  function test_commands_data() {
    return [
      { tag: "zo", key: Qt.Key_O, shift: false, action: "expand" },
      { tag: "zc", key: Qt.Key_C, shift: false, action: "collapse" },
      { tag: "zO", key: Qt.Key_O, shift: true, action: "expand-recursive" },
      { tag: "zC", key: Qt.Key_C, shift: true, action: "collapse-recursive" },
      { tag: "zR", key: Qt.Key_R, shift: true, action: "expand-all" },
      { tag: "zM", key: Qt.Key_M, shift: true, action: "collapse-all" }
    ]
  }
  function test_commands(data) {
    compare(press(Qt.Key_Z), "key-prefix")
    verify(folds.pending)
    if (data.shift) compare(press(Qt.Key_Shift, Qt.ShiftModifier), "key-prefix")
    compare(press(data.key, data.shift ? Qt.ShiftModifier : Qt.NoModifier), data.action)
    verify(!folds.pending)
    compare(press(data.key, data.shift ? Qt.ShiftModifier : Qt.NoModifier, true), "key-prefix")
    compare(press(data.key, data.shift ? Qt.ShiftModifier : Qt.NoModifier), data.key === Qt.Key_O && !data.shift ? "open" : "")
  }
  function test_held_prefix_waits_for_a_new_command_key() {
    press(Qt.Key_Z)
    compare(press(Qt.Key_Z, Qt.NoModifier, true), "key-prefix")
    verify(folds.pending)
    compare(press(Qt.Key_O), "expand")
    compare(press(Qt.Key_Z, Qt.NoModifier, true), "key-prefix")
    verify(!folds.pending)
  }
  function test_escape_and_unknown_continuations_cancel_without_falling_through() {
    for (var key of [Qt.Key_Escape, Qt.Key_D, Qt.Key_X]) {
      press(Qt.Key_Z)
      compare(press(key), "key-cancel")
      verify(!folds.pending)
      compare(press(Qt.Key_O), "open")
    }
  }
  function test_focus_loss_clears_the_prefix_and_leaves_text_input_untouched() {
    press(Qt.Key_Z)
    folds.active = false
    verify(!folds.pending)
    compare(press(Qt.Key_Z), "")
    compare(press(Qt.Key_O), "")
    folds.active = true
    compare(press(Qt.Key_O), "open")
  }
  function test_other_modifiers_are_not_fold_commands() {
    compare(press(Qt.Key_Z, Qt.ShiftModifier), "quicknav")
    compare(press(Qt.Key_Z, Qt.ControlModifier), "")
    press(Qt.Key_Z)
    compare(press(Qt.Key_O, Qt.ControlModifier), "key-cancel")
  }

  function test_custom_bindings_replace_old_keys_and_inherit_other_defaults() {
    folds.plan = KeyBindings.compile({ bindings: { open: ["F3"], expand: ["Space o"], "page-previous": [], help: ["F1"] } })
    compare(press(Qt.Key_F3), "open")
    compare(folds.action({ key: Qt.Key_O, modifiers: 0 }, false, "open"), "key-unbound")
    compare(folds.action({ key: Qt.Key_B, modifiers: Qt.ControlModifier }, false, "page-previous"), "key-unbound")
    compare(press(Qt.Key_Space), "key-prefix")
    compare(press(Qt.Key_O), "expand")
    compare(press(Qt.Key_J), "next")
    compare(press(Qt.Key_F1), "help")
    compare(folds.action({ key: Qt.Key_Question, modifiers: Qt.ShiftModifier }, false, "help"), "key-unbound")
  }
  function test_custom_keys_override_defaults_and_new_prefixes_cancel_on_reload() {
    folds.plan = KeyBindings.compile({ bindings: { collapse: ["l"] } })
    compare(press(Qt.Key_L), "collapse")
    compare(press(Qt.Key_O), "open")
    press(Qt.Key_Z)
    folds.plan = KeyBindings.compile({ bindings: { expand: ["F4"] } })
    verify(!folds.pending)
    compare(press(Qt.Key_O), "open")
  }
  function test_scope_and_range_selection() {
    compare(press(Qt.Key_Down, Qt.ShiftModifier), "next")
    folds.scope = "artifacts"
    compare(press(Qt.Key_Z, Qt.ShiftModifier), "")
    compare(press(Qt.Key_B, Qt.ControlModifier), "page-previous")
    folds.scope = "properties"
    compare(press(Qt.Key_B, Qt.ControlModifier), "page-up")
    compare(press(Qt.Key_J), "scroll-down")
    compare(press(Qt.Key_H), "tree")
    compare(press(Qt.Key_Z), "")
    folds.scope = "files-list"
    compare(press(Qt.Key_Z), "")
    compare(press(Qt.Key_L), "open")
  }
  function test_unknown_metadata_survives_compile_and_json_round_trip() {
    var document = JSON.parse('{"version":1,"filetreeVersion":"999.0.0","bindings":{"open":["F3"]},"future":{"ordered":[null,false,42]},"__proto__":{"keep":true}}')
    var before = JSON.stringify(document)
    folds.plan = KeyBindings.compile(document)
    compare(press(Qt.Key_F3), "open")
    compare(press(Qt.Key_J), "next")
    compare(JSON.stringify(document), before)
    var restored = JSON.parse(JSON.stringify(document))
    folds.plan = KeyBindings.compile(restored)
    compare(press(Qt.Key_F3), "open")
    compare(JSON.stringify(restored), before)
  }

  function test_validation_rejects_ambiguous_and_unversioned_configs() {
    for (var document of [[], {version: 2}, {bindings: []},
      {bindings: {open: ["z"], expand: ["z o"]}}, {bindings: {open: ["F1"], help: ["F1"]}}]) {
      var rejected = false
      try { KeyBindings.compile(document) } catch (_) { rejected = true }
      verify(rejected, JSON.stringify(document))
    }
  }

  function test_unknown_actions_and_bad_bindings_are_dropped_with_a_problem_and_the_rest_still_applies() {
    for (var document of [{bindings: {unknown: ["F1"], open: ["F3"]}}, {bindings: {open: "F1", help: ["F3"]}},
      {bindings: {open: ["Potato", "F3"]}}, {bindings: {open: ["Hyper+O", "F3"]}},
      {bindings: {open: ["a b c d e", "F3"]}}, {bindings: {open: ["z Escape", "F3"]}}]) {
      var plan = KeyBindings.compile(document)
      verify(plan.problems.length > 0, JSON.stringify(document))
      folds.plan = plan
      compare(press(Qt.Key_F3), document.bindings.help ? "help" : "open", JSON.stringify(document))
      compare(press(Qt.Key_J), "next")
    }
    compare(KeyBindings.compile({}).problems, [])
    var kept = KeyBindings.compile({bindings: {open: "F1"}})
    folds.plan = kept
    compare(press(Qt.Key_L), "open")
    compare(kept.custom, [])
  }
}
