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
