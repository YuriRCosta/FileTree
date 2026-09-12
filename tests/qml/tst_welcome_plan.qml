import QtQuick
import QtTest
import "../../modules/welcome/WelcomePlan.js" as WelcomePlan

TestCase {
  name: "WelcomePlan"

  function test_core_and_offline_help_need_no_installer_or_network() {
    compare(WelcomePlan.CORE.map(function(entry) { return entry.id }), ["files", "notes", "skills", "memory", "hooks", "mcp"])
    for (var topic of WelcomePlan.HELP) verify(topic.name.length > 0 && topic.text.length > 40)
    compare(WelcomePlan.LOCAL_CATALOG.version, 1)
    compare(WelcomePlan.LOCAL_CATALOG.entries.length, 0)
    verify(WelcomePlan.installCommand === undefined)
    verify(WelcomePlan.EXTENSIONS === undefined)
  }

  function test_state_pending_only_while_unset() {
    verify(WelcomePlan.pending(""))
    verify(WelcomePlan.pending(undefined))
    verify(!WelcomePlan.pending("dismissed"))
    verify(!WelcomePlan.pending("installed"))
    compare(WelcomePlan.normalizeState("bogus"), "")
    compare(WelcomePlan.welcomeSlot().modules[0].module, "welcome")
    compare(WelcomePlan.welcomeSlot().modules[1].module, "notes")
  }

  function entry(id, contract) {
    return { id: id, name: "Example", source: "https://example.org/source", hostContract: contract, lifecycle: "on-demand" }
  }

  function test_catalog_describes_compatibility_without_commands() {
    var first = entry("example.one/blade", 1)
    first.command = ["sh", "-c", "exit 99"]
    var parsed = WelcomePlan.catalog(JSON.stringify({version: 1, entries: [first, entry("example.two", 4)]}), 3)
    compare(parsed.entries.length, 2)
    compare(parsed.entries[0].id, first.id)
    compare(parsed.entries[0].source, first.source)
    compare(parsed.entries[0].lifecycle, "on-demand")
    verify(parsed.entries[0].compatible)
    verify(!parsed.entries[1].compatible)
    verify(parsed.entries[0].command === undefined)
  }

  function test_malformed_catalogs_are_refused_as_a_whole() {
    for (var text of ["", "{", "null", "[]", '{"version":2,"entries":[]}',
      JSON.stringify({version: 1, entries: [entry("duplicate", 1), entry("duplicate", 1)]}),
      JSON.stringify({version: 1, entries: Array(129).fill(entry("many", 1))})]) {
      compare(WelcomePlan.catalog(text, 3), null)
    }
    for (var change of [{id: "../escape"}, {source: "javascript:alert(1)"}, {source: "file:///tmp/run"},
      {hostContract: 1.5}, {hostContract: -1}, {name: ""}, {name: "\u0001"}, {lifecycle: "installed"}]) {
      var candidate = entry("example", 1)
      for (var key of Object.keys(change)) candidate[key] = change[key]
      compare(WelcomePlan.catalog(JSON.stringify({version: 1, entries: [candidate]}), 3), null)
    }
  }
}
