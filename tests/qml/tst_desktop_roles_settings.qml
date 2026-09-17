import QtQuick
import QtTest
import "../../blades"

TestCase {
  name: "DesktopRolesSettings"
  property var calls: []
  property var statusDocument: ({ schema: 1, action: "roles_status", roles: {} })
  property var setDocument: ({})

  Item {
    id: mockHost
    property bool nativeApp: true
    function rolesStatus(callback) { calls.push({ command: "roles-status" }); callback(statusDocument); return "status" }
    function setRole(role, on, callback) { calls.push({ command: "roles-set", role: role, on: on }); callback(setDocument); return "set" }
  }

  Component { id: component; BladeDesktopRoles { host: mockHost; width: 400 } }

  function status(enabled, extra) {
    var roles = {}
    var names = ["folder", "reveal", "chooser", "bindings", "autostart"]
    for (var i = 0; i < names.length; i++) {
      roles[names[i]] = { enabled: enabled.indexOf(names[i]) >= 0, launcher: "/tmp/fixture-home/.local/bin/fileblade", conflict: "", entries: [] }
      if (extra && extra[names[i]]) for (var key in extra[names[i]]) roles[names[i]][key] = extra[names[i]][key]
    }
    return { schema: 1, action: "roles_status", roles: roles }
  }

  function setResult(role, state, error) {
    var roles = {}
    roles[role] = { status: state, error: error || "", remaining_owned_entries: [] }
    return { schema: 1, action: state === "already_on" ? "roles_enable" : "roles_disable", status: "complete", error: "", roles: roles }
  }

  function init() {
    calls = []
    statusDocument = status(["reveal"])
    setDocument = setResult("folder", "enabled")
  }

  function row(section, role) { return findChild(section, "role-" + role) }
  function detail(section, role) { return findChild(section, "detail-" + role).text }

  function test_rows_render_from_the_status_document() {
    var section = createTemporaryObject(component, this)
    verify(section.nativeApp)
    compare(calls.length, 1)
    compare(calls[0].command, "roles-status")
    var labels = ["Open folders with FileBlade", "Reveal in FileBlade", "File chooser", "Hyprland bindings", "Start at login"]
    var names = ["folder", "reveal", "chooser", "bindings", "autostart"]
    for (var i = 0; i < names.length; i++) {
      compare(row(section, names[i]).label, labels[i])
      compare(row(section, names[i]).checked, names[i] === "reveal")
    }
    compare(detail(section, "folder"), "Makes FileBlade the inode/directory handler in mimeapps.list")
    verify(section.keywords.indexOf("autostart login") >= 0)
    verify(section.keywords.indexOf("Start at login") >= 0)
  }

  function test_toggling_calls_set_role_and_shows_the_result() {
    var section = createTemporaryObject(component, this)
    setDocument = setResult("folder", "enabled")
    statusDocument = status(["folder", "reveal"])
    row(section, "folder").toggled()
    compare(calls[1].command, "roles-set")
    compare(calls[1].role, "folder")
    compare(calls[1].on, true)
    compare(calls[2].command, "roles-status")
    verify(row(section, "folder").checked)
    compare(detail(section, "folder"), "Makes FileBlade the inode/directory handler in mimeapps.list")

    setDocument = setResult("reveal", "restored")
    statusDocument = status(["folder"])
    row(section, "reveal").toggled()
    compare(calls[3].role, "reveal")
    compare(calls[3].on, false)
    verify(!row(section, "reveal").checked)
    compare(detail(section, "reveal"), "restored the previous handler")

    setDocument = setResult("folder", "preserved_newer")
    statusDocument = status([])
    row(section, "folder").toggled()
    compare(detail(section, "folder"), "kept your newer choice")
    compare(detail(section, "reveal"), "Serves org.freedesktop.FileManager1 for other applications' Reveal actions")
  }

  function test_conflict_and_errors_show_on_the_row() {
    var section = createTemporaryObject(component, this)
    var conflict = "nautilus currently owns org.freedesktop.FileManager1; log out and in for FileBlade to take over"
    setDocument = setResult("reveal", "enabled")
    statusDocument = status(["reveal"], { reveal: { conflict: conflict } })
    row(section, "reveal").toggled()
    compare(detail(section, "reveal"), conflict)

    setDocument = setResult("chooser", "refused", "no stable launcher")
    statusDocument = status(["reveal"], { reveal: { conflict: conflict } })
    row(section, "chooser").toggled()
    compare(detail(section, "chooser"), "no stable launcher")

    setDocument = { ok: false, error: "Backend request failed" }
    row(section, "autostart").toggled()
    compare(detail(section, "autostart"), "Backend request failed")
  }

  function test_section_is_absent_when_not_native() {
    mockHost.nativeApp = false
    var section = createTemporaryObject(component, this)
    verify(!section.nativeApp)
    verify(!section.visible)
    compare(calls.length, 0)
    row(section, "folder").toggled()
    compare(calls.length, 0)
    mockHost.nativeApp = true
  }
}
