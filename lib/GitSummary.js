.pragma library

var choices = [
  { key: "branch", label: "Branch", glyph: "" },
  { key: "worktree", label: "Worktree", glyph: "󰙅" },
  { key: "ahead", label: "Commits ahead", glyph: "↑" },
  { key: "behind", label: "Commits behind", glyph: "↓" },
  { key: "modified", label: "Modified files", glyph: "M" },
  { key: "added", label: "Added files", glyph: "A" },
  { key: "untracked", label: "Untracked files", glyph: "?" },
  { key: "deleted", label: "Deleted files", glyph: "D" },
  { key: "renamed", label: "Renamed files", glyph: "R" },
  { key: "copied", label: "Copied files", glyph: "C" },
  { key: "type_changed", label: "Type changes", glyph: "T" },
  { key: "conflicted", label: "Conflicts", glyph: "U" }
]

function normalizeFields(value) {
  if (value === undefined || value === null) return choices.map(function(choice) { return choice.key })
  var values = Array.isArray(value) ? value : String(value).split(",")
  return choices.filter(function(choice) { return values.indexOf(choice.key) >= 0 }).map(function(choice) { return choice.key })
}

function summaryRow(isGitRepo, depth, path, rootPath, fields) {
  return !!isGitRepo && Number(depth) === 0 && String(path || "") !== ""
    && String(path) === String(rootPath || "") && normalizeFields(fields).length > 0
}

function describe(raw, fields) {
  var enabled = normalizeFields(fields)
  var summary
  try { summary = JSON.parse(raw || "null") } catch (_) { summary = null }
  if (!summary) return { text: "…", tokens: [{ text: "…", marker: "" }], tooltip: "Loading repository status" }
  if (!summary.ok) return { text: "Unavailable", tokens: [{ text: "Unavailable", marker: "U" }], tooltip: "Repository status is unavailable; refresh to retry" }
  var parts = [], tokens = [], labels = [], tooltipTokens = []
  function label(text, marker) { labels.push(text); tooltipTokens.push({ text: text, marker: marker || "" }) }
  var identity = []
  if (summary.branch) {
    label("Branch / HEAD: " + summary.branch)
    if (enabled.indexOf("branch") >= 0) identity.push(" " + summary.branch)
  }
  if (summary.worktree) {
    label("Worktree: " + summary.worktree)
    if (enabled.indexOf("worktree") >= 0) identity.push("󰙅 " + summary.worktree)
  }
  function add(text, marker) { parts.push(text); tokens.push({ text: text, marker: marker }) }
  if (summary.ahead !== null && summary.behind !== null) {
    if (enabled.indexOf("ahead") >= 0) add("↑" + summary.ahead, summary.ahead > 0 ? "ahead" : "")
    if (enabled.indexOf("behind") >= 0) add("↓" + summary.behind, summary.behind > 0 ? "behind" : "")
    if (summary.ahead > 0) label(summary.ahead + " commits ahead of " + summary.upstream + " (last fetched state)", "ahead")
    if (summary.behind > 0) label(summary.behind + " commits behind " + summary.upstream + " (last fetched state)", "behind")
  } else {
    label(summary.upstream ? "Upstream comparison unavailable" : "No upstream comparison (no upstream or detached HEAD)")
  }
  var changes = [["modified", "M", "modified"], ["added", "A", "added"], ["untracked", "?", "untracked"],
    ["deleted", "D", "deleted"], ["renamed", "R", "renamed"], ["copied", "C", "copied"],
    ["type_changed", "T", "type changed"], ["conflicted", "U", "conflicted"]]
  for (var i = 0; i < changes.length; i++) {
    var count = Number(summary[changes[i][0]]) || 0
    if (count > 0) {
      label(count + " " + changes[i][2], changes[i][1])
      if (enabled.indexOf(changes[i][0]) >= 0) add(changes[i][1] + count, changes[i][1])
    }
  }
  return { text: parts.join(" "), identity: identity.join(" "), tokens: tokens, tooltip: labels.join("\n"), tooltipTokens: tooltipTokens }
}
