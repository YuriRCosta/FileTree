import QtQuick
import QtTest
import "../../lib/PathText.js" as PathText

TestCase {
  name: "OperationsForms"

  function test_forms_preserve_paths_validate_input_and_only_enqueue_on_submit() {
    var xhr = new XMLHttpRequest()
    xhr.open("GET", Qt.resolvedUrl("../../panes/FileActionsMenu.qml"), false)
    xhr.send()
    var source = xhr.responseText
    verify(source.length > 0)
    var methods = source.slice(source.indexOf("  function prepare()"), source.indexOf("  function isPaletteColor("))
    var calls = []
    var closed = 0
    var root = { visible: true, query: "", archiveMode: true, permissionsMode: false, customColorMode: false,
      paths: ["/source/space #percent%.txt", "/other/-file"], archiveFormat: "tar.zst",
      focusCurrent: function() {}, returnToTree: function() { closed++ } }
    var controller = { actionMenuMode: "archive-create", actionInput: "", operationError: "",
      focusTree: function() {},
      rootName: function(path) { return path.split("/").pop() },
      backendCommand: function(kind) { return ["/fileblade", "_backend", kind] },
      history: { enqueue: function(label, args) { calls.push(args) } } }
    var prepare = new Function("root", "controller", "PathText", "Qt", methods + "\nprepare()")
    var submit = new Function("root", "controller", "PathText", methods + "\nsubmitInput()")
    prepare(root, controller, PathText, { callLater: function() {} })
    compare(controller.actionInput, "/source/space #percent%.txt.tar.zst")
    compare(calls.length, 0)
    controller.actionInput = "relative.zip"
    submit(root, controller, PathText)
    compare(calls.length, 0)
    verify(controller.operationError.indexOf("absolute") >= 0)
    controller.actionInput = "/target/space #%.zip"
    root.archiveFormat = "zip"
    submit(root, controller, PathText)
    compare(calls[0], ["/fileblade", "_backend", "archive-create", "--destination", "/target/space #%.zip",
      "--format", "zip", "--source", root.paths[0], "--source", root.paths[1]])
    compare(closed, 1)
    root.archiveMode = false
    root.permissionsMode = true
    controller.actionMenuMode = "permissions-set"
    prepare(root, controller, PathText, { callLater: function() {} })
    compare(controller.actionInput, "")
    for (var invalid of ["", "7777", "999", "u+x"]) {
      controller.actionInput = invalid
      submit(root, controller, PathText)
      compare(calls.length, 1)
    }
    controller.actionInput = "640"
    submit(root, controller, PathText)
    compare(calls[1], ["/fileblade", "_backend", "permissions-set", "--mode", "640", "--path", root.paths[0], "--path", root.paths[1]])
    compare(closed, 2)
  }
}
