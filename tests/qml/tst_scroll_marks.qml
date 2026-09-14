import QtQuick
import QtTest
import "../../lib/GitSummary.js" as GitSummary
import "../../lib/ScrollMarks.js" as ScrollMarks

TestCase {
  name: "ScrollMarks"

  ListModel { id: model }

  function fill(statuses) {
    model.clear()
    for (var i = 0; i < statuses.length; i++) {
      var entry = statuses[i]
      var status = typeof entry === "string" ? entry : entry.status
      model.append({ path: "/p/" + i, gitStatus: status, gitIgnored: typeof entry === "object" && !!entry.ignored })
    }
  }

  function statuses(marks) { return marks.map(function(mark) { return mark.status }) }

  function test_marks_sit_at_the_row_fraction() {
    fill(["", "M", "", "?"])
    var marks = ScrollMarks.collect(model, 100)
    compare(statuses(marks), ["M", "?"])
    compare(marks[0].fraction, 1.5 / 4)
    compare(marks[1].fraction, 3.5 / 4)
    compare(marks[0].index, 1)
  }

  function test_ignored_unknown_and_clean_rows_leave_no_mark() {
    fill(["", { status: "M", ignored: true }, "!", "X", "D"])
    compare(statuses(ScrollMarks.collect(model, 100)), ["D"])
  }

  function test_the_opened_repository_row_leaves_no_mark_behind_its_summary() {
    var fields = ["branch", "untracked"]
    var rows = [
      { path: "/repo", depth: 0, isGitRepo: true, gitStatus: "?", gitIgnored: false },
      { path: "/repo/.agents", depth: 1, isGitRepo: false, gitStatus: "?", gitIgnored: false },
      { path: "/repo/.opencode", depth: 1, isGitRepo: false, gitStatus: "?", gitIgnored: false }
    ]
    function status(row) {
      if (!row || row.gitIgnored) return ""
      if (GitSummary.summaryRow(row.isGitRepo, row.depth, row.path, "/repo", fields)) return ""
      return String(row.gitStatus || "")
    }
    compare(ScrollMarks.collect(rows, 100, status).length, 2)
    compare(ScrollMarks.collect(rows, 100).length, 3)
    compare(GitSummary.summaryRow(true, 0, "/repo", "/repo", []), false)
    compare(GitSummary.summaryRow(true, 0, "/repo/child", "/repo", fields), false)
    compare(GitSummary.summaryRow(false, 0, "/repo", "/repo", fields), false)
  }

  function test_one_slot_keeps_the_most_severe_status() {
    fill(["R", "M", "?", "D", "A", "M"])
    compare(statuses(ScrollMarks.collect(model, 1)), ["D"])
    compare(statuses(ScrollMarks.collect(model, 2)), ["M", "D"])
    compare(ScrollMarks.collect(model, 2)[0].index, 1)
  }

  function test_view_range_follows_the_viewport_in_content_coordinates() {
    var range = ScrollMarks.viewRange(100, 0, 50, 400)
    compare(range.start, 0.25)
    compare(range.end, 0.375)
    range = ScrollMarks.viewRange(10, -90, 50, 400)
    compare(range.start, 0.25)
    compare(range.end, 0.375)
    verify(ScrollMarks.inView(0.3, range))
    verify(!ScrollMarks.inView(0.5, range))
    verify(!ScrollMarks.inView(0.1, range))
    compare(ScrollMarks.viewRange(0, 0, 50, 0).end, 1)
  }

  function test_empty_inputs_return_nothing() {
    fill([])
    compare(ScrollMarks.collect(model, 10).length, 0)
    fill(["M"])
    compare(ScrollMarks.collect(model, 0).length, 0)
    compare(ScrollMarks.collect(null, 10).length, 0)
  }
}
