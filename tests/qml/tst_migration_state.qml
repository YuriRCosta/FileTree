import QtQuick
import QtTest
import "../../lib/StateDocument.js" as StateDocument

TestCase {
  name: "MigrationState"

  function test_unknown_fields_survive_known_preference_updates() {
    var original = { version: 12, showHidden: false, future: { ordered: [false, null, "keep"] }, extensionChoice: 42 }
    var merged = StateDocument.mergeFields(original, { version: 12, showHidden: true })
    compare(merged.showHidden, true)
    compare(merged.extensionChoice, 42)
    compare(JSON.stringify(merged.future), JSON.stringify(original.future))
    merged.future.ordered.push("new")
    compare(original.future.ordered.length, 3)
  }

  function test_repeated_save_preserves_unrecognized_fields_and_data_keys() {
    var original = JSON.parse('{"version":12,"future":{"enabled":false},"__proto__":{"keep":true}}')
    var first = StateDocument.mergeFields(original, { version: 12, rootPath: "/tmp/first" })
    var second = StateDocument.mergeFields(first, { version: 12, rootPath: "/tmp/second" })
    compare(JSON.stringify(second.future), '{"enabled":false}')
    compare(JSON.stringify(second["__proto__"]), '{"keep":true}')
    verify(Object.prototype.hasOwnProperty.call(second, "__proto__"))
    verify(second.keep === undefined)
    compare(second.rootPath, "/tmp/second")
  }

  function test_absent_document_uses_current_fields() {
    compare(JSON.stringify(StateDocument.mergeFields(null, { version: 12 })), '{"version":12}')
    compare(JSON.stringify(StateDocument.mergeFields([], { version: 12 })), '{"version":12}')
  }
}
