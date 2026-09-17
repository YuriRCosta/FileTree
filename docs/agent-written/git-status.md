This file was written by an agent.

# Git status

Only the opened repository's root row replaces its columns with a summary.
Dimmed branch and linked-worktree names appear on the left of the counts, separated
by a space. The main checkout has no worktree label, including in Properties.
Properties lists each Git status on a separate line, keeping adjacent rows aligned.
The summary ends at the
right edge of the first detail column, or the name column when no detail is selected.

Markers are consistent across the tree and summary: `M` modified, `A` added,
`?` untracked, `D` deleted, `R` renamed, `C` copied, `T` type changed, and `U` conflicted.
Counts follow the marker. Zero file counts are omitted. `↑` and `↓` compare
commits with the configured upstream using locally available refs; displaying
them never fetches from the network. Hover for color-matched status names and
nonzero counts, without the redundant word “files”.

Under **Files settings → Git**, each summary field can be toggled independently.
Choices persist across restarts. Disabling every field restores the normal columns.
The same selection is available from the CLI:

```sh
fileblade git-summary branch worktree ahead behind modified added untracked deleted renamed copied type_changed conflicted
fileblade git-summary branch modified untracked
fileblade git-summary
```

The last command hides the summary. Changing display preferences does not
change Git data, sorting, staging, or repository contents.

## Branches

The Switch branch popup on the footer branch name starts with `Expand into
Branches`, which opens the Branches module as a tab beside Properties, wherever
Properties sits; without a Properties slot it takes its own slot under Files at
about a third of the height. `fileblade branches` opens or focuses it, `fileblade branches close`
removes it, and `fileblade branches list [-o json]` prints the same document
in the terminal. First-use layouts include Branches as the second tab behind
Properties, with Properties active. Saved layouts are preserved, including
when Branches has been dismissed. It also appears in Add module.

The module reads one backend document, `git-places`, for the repository the
tree's context path resolves to: every local and remote branch merged by short
name (`local`, `remote` or `both`), its upstream with ahead and behind counts,
the worktree it is checked out in, its tip commit date, author and subject,
and every worktree with its head, lock state, staged, changed, untracked and
conflicted counts, and the same per-letter change counts (`modified`, `added`,
`untracked`, `deleted`, `renamed`, `copied`, `type_changed`, `conflicted`) the
tree's repository summary uses. Nothing is fetched from the network and
nothing is written.

Rows are grouped as Branches, Branches / Remote, then Worktrees. The main
checkout is never a worktree row: it is the checked-out branch. A linked
worktree nests under the branch it has checked out, open by default, with its
path as summary; a detached worktree lands in the Worktrees group. The Status
column, on by default, uses the tree's repository summary format and colours
(`↑3 ↓1 M4 A1 ?2`, honouring the Git summary fields setting): a branch shows
its upstream arrows plus the changes of its checkout, a worktree row shows its
changes plus `locked`, and `remote`, `gone`, `no upstream` or `clean` stand
in when there is nothing to show. Kind, Updated, Author and Summary are the
other choices. Enter on a branch runs `git-switch` (a remote-only branch is
tracked from its single remote), then refreshes the module and the tree's
Git metadata; a refusal shows in the header. Enter on a branch with a linked
worktree folds it instead, since Git refuses to switch to it. Enter on a
worktree opens it in the tree, and `o` on a branch checked out elsewhere
opens that worktree. Selecting a checked-out branch or worktree first looks
up its path in the Files tree index and selects that row, otherwise navigating
to the checkout. Remote-only selection never navigates. Local branch glyphs
use the theme accent; remote glyphs are muted. A separate mark identifies
the current checkout. The header shows its branch and summary with a Git glyph.

Refreshes on open, context changes, Git metadata updates and Shift+R are
coalesced for 120 ms. Superseded requests are cancelled through the host and
generation checks reject late responses. Closing the pane cancels its work.
Git status is collected once per checkout, never once per branch.
