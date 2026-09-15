import QtQuick
import QtTest
import "../../lib/GitSummary.js" as GitSummary

TestCase {
  name: "RepositorySummary"

  function test_repository_states() {
    var data = { ok: true, ahead: 2, behind: 3, upstream: "origin/main", modified: 4, added: 2, untracked: 3, copied: 1, type_changed: 1, deleted: 6, renamed: 1, conflicted: 2 }
    var result = GitSummary.describe(JSON.stringify(data))
    compare(result.text, "↑2 ↓3 M4 A2 ?3 D6 R1 C1 T1 U2")
    compare(result.tokens.map(function(part) { return part.marker }).join(","), "ahead,behind,M,A,?,D,R,C,T,U")
    verify(result.tooltip.indexOf("last fetched state") >= 0)
    verify(result.tooltip.indexOf("2 added") >= 0)
    verify(result.tooltip.indexOf("3 untracked") >= 0)
    verify(result.tooltip.indexOf("files") < 0)
    compare(result.tooltipTokens.filter(function(part) { return part.marker === "?" })[0].text, "3 untracked")
    data.ahead = null; data.behind = null
    verify(GitSummary.describe(JSON.stringify(data)).text.indexOf("↑") < 0)
    compare(GitSummary.describe(JSON.stringify({ ok: true, ahead: 0, behind: 0, upstream: "origin/main" })).text, "↑0 ↓0")
    compare(GitSummary.describe(JSON.stringify({ ok: true, ahead: null, behind: null, upstream: "" })).text, "")
    compare(GitSummary.describe('{"ok":false}').text, "Unavailable")
    compare(GitSummary.describe("").text, "…")
  }

  function test_display_choices_preserve_identity_and_drop_unknown_fields() {
    var raw = JSON.stringify({ ok: true, branch: "feature/<test>", worktree: "review", ahead: 1, behind: 0, upstream: "origin/main", modified: 2, untracked: 3 })
    var result = GitSummary.describe(raw, ["branch", "worktree", "untracked"])
    compare(result.identity, " feature/<test> 󰙅 review")
    compare(result.text, "?3")
    compare(GitSummary.describe(raw, []).identity, "")
    compare(GitSummary.describe(raw, []).text, "")
    compare(GitSummary.normalizeFields(["branch", "branch", "invalid", "added"]).join(","), "branch,added")
    compare(GitSummary.normalizeFields("").length, 0)
    compare(GitSummary.normalizeFields().length, GitSummary.choices.length)
  }

  function test_main_checkout_has_no_worktree_label() {
    var result = GitSummary.describe(JSON.stringify({ ok: true, branch: "main", worktree: "", ahead: null, behind: null }))
    compare(result.identity, " main")
    verify(result.tooltip.indexOf("Worktree:") < 0)
  }

  function test_tooltip_omits_zero_counts() {
    var result = GitSummary.describe(JSON.stringify({ ok: true, branch: "main", ahead: 0, behind: 0, upstream: "origin/main", modified: 2, added: 0, untracked: 0 }))
    compare(result.tooltip, "Branch / HEAD: main\n2 modified")
    compare(result.tooltipTokens[1].marker, "M")
  }
}
