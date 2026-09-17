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
Branches`, which opens the Branches module in the left blade under Files (or
under Properties when Properties is under Files) at about a third of the
height. `fileblade branches` opens or focuses it, `fileblade branches close`
removes it, and `fileblade branches list [-o json]` prints the same document
in the terminal. It is not part of the default layout, but it persists once
opened and appears in Add module.

The module reads one backend document, `git-places`, for the repository the
tree's context path resolves to: every local and remote branch merged by short
name (`local`, `remote` or `both`), its upstream with ahead and behind counts,
the worktree it is checked out in, its tip commit date, author and subject,
and every worktree with its head, lock state and staged, changed, untracked
and conflicted counts. Nothing is fetched from the network and nothing is
written.

Rows are grouped as Worktrees, Branches and Branches / Remote. The Status
column is on by default; Kind, Updated, Author and Summary are the other
choices. Enter on a branch runs `git-switch` (a remote-only branch is tracked
from its single remote), then refreshes the module and the tree's Git
metadata; a refusal shows in the header. Enter on a worktree opens it in the
tree, and `o` on a branch checked out elsewhere opens that worktree. The list
reloads on open, on a context path change, after the tree's Git refresh and
on Shift+R.
