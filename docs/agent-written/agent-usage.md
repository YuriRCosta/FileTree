This file was written by an agent.

# Agent usage history

How the Skills and MCP blades count skill and MCP use, where that history is
kept, and how the activity heatmap and `fileblade usage` read it.

Coding agents already write a transcript of every session. FileBlade reads
those transcripts, keeps one row per skill call, typed command and MCP call in
a private SQLite store, and answers the Uses columns, the observed MCP rows and
the daily heatmap from that store. Events outlive the transcripts they came
from, so the store holds history that cannot be rebuilt once an agent deletes
old transcripts.

The code is the Python package `python/agent_usage/`:

```yaml
records.py:  turns one Claude Code or Codex transcript record into events
store.py:    store path, schema, lock, and the budgeted transcript ingest
query.py:    name matching, the list totals, the daily history and forget
```

## The store

```yaml
path:         $XDG_STATE_HOME/omarchy/fileblade/agent-usage.sqlite3
              (XDG_STATE_HOME falls back to ~/.local/state; a relative value is ignored)
lock:         agent-usage.sqlite3.lock beside it
permissions:  directory created 0700, database and lock created 0600; SQLite creates the
              -wal and -shm files with the database's permissions
engine:       Python standard library sqlite3, WAL journal, busy_timeout 5000
version:      PRAGMA user_version = 2. Version 1 gains retention, pending-failure and forgotten-identity tables without losing history.
              Unknown versions are refused; existing tables are never dropped
old cache:    $XDG_CACHE_HOME/omarchy/fileblade/agent-usage.json and agent-usage.tmp are
              deleted whenever the store opens. Nothing is migrated from them
```

It is state, not cache: a cache cleaner must not erase history. Uninstalling
the plugin keeps it with the rest of `~/.local/state/omarchy/fileblade/`.

```sql
CREATE TABLE source (
  id INTEGER PRIMARY KEY,
  agent TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  device INTEGER NOT NULL,
  inode INTEGER NOT NULL,
  size INTEGER NOT NULL,
  mtime INTEGER NOT NULL,
  offset INTEGER NOT NULL,
  project INTEGER REFERENCES project(id)
);
CREATE TABLE project (id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE);
CREATE TABLE coverage (agent TEXT PRIMARY KEY, first_at INTEGER NOT NULL);
CREATE TABLE event (
  agent TEXT NOT NULL,
  call TEXT NOT NULL,
  at INTEGER NOT NULL,
  kind TEXT NOT NULL,
  origin TEXT NOT NULL,
  server TEXT NOT NULL DEFAULT '',
  name TEXT NOT NULL,
  subagent INTEGER NOT NULL DEFAULT 0,
  project INTEGER REFERENCES project(id),
  failed INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (agent, call)
) WITHOUT ROWID;
CREATE INDEX event_time ON event (kind, at);
CREATE INDEX event_name ON event (kind, server, name, at);
CREATE TABLE retention (id INTEGER PRIMARY KEY CHECK (id = 1), before INTEGER NOT NULL);
CREATE TABLE failure (call TEXT PRIMARY KEY, at INTEGER NOT NULL) WITHOUT ROWID;
CREATE TABLE forgotten (identity BLOB PRIMARY KEY) WITHOUT ROWID;
```

```yaml
source:    one row per transcript file: identity, size, mtime in nanoseconds, and the byte
           offset read so far. Codex rollouts also carry their session's project
project:   working directories seen in transcripts
coverage:  per agent, the earliest record timestamp ever read. Kept as the minimum across
           runs, so deleting old transcripts never moves it later
retention: one monotonic UTC millisecond cutoff below which ingest cannot insert events
forgotten: SHA-256 digests of forgotten call identities with future timestamps, so a bad clock
           cannot either replay those calls or move the retention cutoff into the future
failure:   failure call IDs and timestamps awaiting their Claude call, allowing newest-first
           ingestion to read a result in a resumed transcript before its original call
event:     one row per use. at is UTC epoch milliseconds
kind:      skill | command | tool | resource-list | resource
origin:    agent | user | scheduled
call:      the tool_use id, the Codex item id, or the record uuid for a typed command
           (suffixed :1, :2 and so on when one record holds several commands)
```

## What is recorded

Claude Code transcripts:

```yaml
skill:          assistant tool_use named Skill: kind skill, origin agent, name = input.skill
                exactly as written, which may be <plugin>:<skill>
tool:           tool_use named mcp__<server>__<tool>: kind tool, origin agent, server = the
                second segment, name = the rest joined by "__"
resource-list:  tool_use ListMcpResourcesTool: server = input.server or "", name ""
resource:       tool_use ReadMcpResourceTool or ReadMcpResourceDirTool: server =
                input.server as written, name = input.uri without its query or fragment,
                cut to 512 UTF-8 bytes
command:        user record text containing <command-name>/NAME</command-name>, NAME made of
                letters, digits, ":", "_" and "-": kind command, name without the slash,
                origin scheduled when the record has scheduledTaskId, else user. Every
                command is stored, skill or not; classification happens at query time
failure:        dated user tool_result with is_error true marks the matching claude event failed;
                unmatched failures wait for their call, then the pending row is removed
subagent:       1 when isSidechain is true
project:        the record's cwd
```

Codex rollouts:

```yaml
tool:     event_msg whose payload is item_completed with an McpToolCall item: kind tool,
          origin agent, server = item.server (the configured name), name = item.tool,
          failed when item.status is not "completed" or item.result.isError is true
project:  session_meta payload.cwd
```

A record without a usable timestamp, tool_use id, item id or record uuid is
skipped.

Never recorded: tool arguments, skill arguments, command arguments, tool
results, message text, thinking, token counts, resource query strings and
fragments, and the `attributionSkill`, `attributionMcpServer` and
`attributionMcpTool` fields. Those attribution fields describe context rather
than the call, lag behind the real call, and are left alone; a prompt's
follow-up records carrying `attributionSkill: mcp__<server>__<prompt>` are not
a skill use.

## Reading transcripts

There is no transcript watcher and no background process. Every helper call
that reads usage (`list` and `usage`) first runs one
budgeted ingest in its own process, then answers from what is committed. A
blade refresh runs `list` for its project and user lanes and one `usage`
request when an activity view is visible, so new transcript bytes arrive with the next refresh of an open
Skills or MCP tab, after a mutation, or when the tab's anchor folder changes.

```yaml
roots:        claude: $CLAUDE_CONFIG_DIR/projects, else ~/.claude/projects, every *.jsonl
              below, subagent transcripts included
              codex: $CODEX_HOME/sessions and $CODEX_HOME/archived_sessions, else ~/.codex/...
walk cap:     8192 regular files per agent, taken in directory walk order
order:        newest mtime first
lock:         non-blocking flock on the lock file, retried every 50 ms for up to 4 s. A helper
              that never gets it skips ingest and reports pending
budget:       3 s from the start of ingest, lock wait included. Checked between records,
              including pieces of oversized records; unfinished work reports pending
unchanged:    a fully read file whose device, inode, size and mtime match its source row is skipped
offsets:      reading resumes at the stored offset. A changed device or inode, or a size below
              the offset, restarts the file at byte 0
partial line: reading stops at the first line without a trailing newline, so a half-written
              record waits for the next call
record cap:   lines over 4 MiB are skipped using bounded reads; offsets inside these discarded
              lines resume skipping until their newline. Malformed or excessively nested JSON is skipped
prefilter:    Claude lines are parsed only when they contain "Skill", mcp__, McpResource,
              command-name or "is_error" (regardless of JSON spacing); Codex lines only when they contain
              McpToolCall. On a read from byte 0, lines carrying "timestamp" are also parsed
              until the first record with a real timestamp, so coverage starts where the
              file does
transaction:  BEGIN IMMEDIATE per chunk (8 MiB or 1024 events, plus at most one record):
              INSERT OR IGNORE events, failure updates, coverage and the source offset commit
              together. Large files commit progress within a call and after budget expiry.
              Every chunk checks retention inside the transaction, including when forget races it
duplicates:   the (agent, call) key makes re-reading idempotent and drops a command copied into
              a resumed session
unreadable:   files that cannot be stat'ed or opened are counted and reported, never fatal
vanished:     after a scan that finished within budget, source rows whose file no longer exists
              are deleted. Events are never deleted by ingest
```

Measured during the review with 1,008 transcripts (8.28 GiB): the revised
store completed in calls of 3.02 s, 3.01 s, 3.01 s and 1.41 s during concurrent
validation; a warm call took 0.02 s. An earlier run of the previous reader
took 3.04 s and 2.02 s under different load, so these are not a controlled
speed comparison. Both produced identical event totals (233 commands, 104
skill calls and 788 tool calls). Chunk commits bound recovery work if a
helper is killed during a large file.

## Name matching

Matching lives in `query.py` and is shared by `list` and `usage`.

```yaml
sanitize(name):   re.sub(r"[^a-zA-Z0-9_-]", "_", name); for a name starting with
                  "claude.ai " also collapse runs of "_" and strip them from both ends.
                  This is the rule Claude Code 2.1.258 uses to build mcp__ tool names
skill row:        events of kind skill or command named exactly the row name, or
                  <plugin>:<row name> when the row source is plugin:<plugin>@<marketplace>
mcp, claude:      the row's event server is sanitize(name). A plugin-scope row uses
                  plugin_<sanitize(plugin)>_<sanitize(name)>, using source.plugin from the
                  manifest name or installed registry identity, independent of cache layout
                  Stored Claude servers are sanitized before comparison, so a resource call
                  recorded with input.server "my.server" or "plugin:toolkit:docs" meets the
                  same row as mcp__my_server__ or mcp__plugin_toolkit_docs__ tool calls
mcp, codex:       the event server equals the configured name
mcp, other:       definitions of any other agent never match and report 0
ambiguity:        two or more definitions of one agent resolving to the same event server all
                  report 0, carry usageAmbiguous true and list no observed entries. MCP scans
                  all scopes before filtering the requested lane, so splitting the UI into
                  project and user lanes cannot hide a collision
mcp prompt:       a claude command named mcp__<server>__<prompt> counts as kind prompt for that
                  server, origin user or scheduled
```

Totals, for a skill row and for an MCP definition:

```yaml
usesAgent:      skill: skill events. mcp: tool, resource and resource-list events
usesUser:       skill: command events with origin user. mcp: prompt events with origin user
usesScheduled:  command or prompt events with origin scheduled
uses:           usesAgent + usesUser; scheduled runs are left out
failed:         skill: failed skill events. mcp: failed events of any kind
```

## Helper methods

Both helpers are core-module inventory helpers, reached through the resident
backend's `helper-read` and `helper-write` requests. `CoreRoute::permits` in
`src/module_helpers.rs` admits `usage` as a read for Skills and MCP and
`usage-forget` as a write for MCP only; both modules' `source.json` declare the
same methods. A store failure (an `OSError` or `sqlite3.Error`) never fails a
whole inventory.

### `list` (both)

Unchanged payload shape. Each row carries `uses`, `usesAgent`, `usesUser`,
`usesScheduled` and `failed` at top level and inside `metrics`. The document
adds:

```yaml
usageTranscripts:    source rows for the agents that feed the helper (skills: claude;
                     mcp: claude and codex)
usageUnreadable:     files this call could not read
usageIngestPending:  true when the budget or the lock left transcripts unread
usageAmbiguous:      mcp only, the number of ambiguous definitions, 0 when none
usageError:          "usage store unavailable" in place of the fields above when the store
                     failed; rows then have no counts
```

When the store answers, every MCP definition also carries `observed`, `[]`
when empty: up to 64 entries sorted by uses descending, then name, then kind.

```json
{"kind": "tool", "name": "execute_csharp_script", "uses": 133, "failed": 1, "lastUsed": "2026-09-14"}
```

`kind` is `tool`, `resource`, `resource-list` or `prompt`; `name` is `""` for a
resource list; `uses` leaves scheduled prompt runs out, so a prompt only a
timer ran is listed with `uses` 0; `lastUsed` is the local date of the newest
event.

### `usage` (both, read)

```text
agent-skillsctl usage --json --project PATH [--exact]
agent-mcpctl usage --json
```

```json
{"ok": true, "schemaVersion": 1, "kind": "skill", "coverageStart": "2026-05-06",
 "until": "2026-09-16", "ingestPending": false,
 "days": [["2026-09-14", 6, 4, 2, 1, 0]]}
```

```yaml
kind:           "skill" or "mcp"
days[]:         [local date, uses, agent, user, scheduled, failed], only days with at least
                one event, ascending, limited to the 160 weeks (1120 days) ending today
local date:     date(at / 1000, 'unixepoch', 'localtime'), so TZ in the helper's environment
                decides the day
coverageStart:  local date of the earliest coverage.first_at of the contributing agents
                (skill: claude; mcp: claude and codex), null when nothing was ever read
until:          today's local date
skill days:     every skill event of any name, plus command events whose name matches a skill
                discovered for --project (scope all, same matching as list)
mcp days:       every tool, resource and resource-list event of every agent, plus claude
                commands named mcp__<server>__<prompt>
failure:        {"ok": false, "schemaVersion": 1, "kind": ..., "error": "usage store
                unavailable"}, exit status 1
```

The helper starts in the app root, not the caller's directory, so the skills
project always arrives through `--project`.

### `usage-forget` (MCP helper, write)

```text
agent-mcpctl usage-forget [--before YYYY-MM-DD] --json
```

```yaml
with --before:  deletes events before local midnight at the start of that day, raises coverage
                to that moment and records a retention cutoff
without:        deletes every event, coverage row and project row (source.project is cleared).
                Pending failure IDs are removed too. The cutoff advances through now; digests
                of any already-recorded future call identities prevent their replay
privacy:        PRAGMA secure_delete = ON overwrites deleted SQLite cells. No post-commit VACUUM
                can turn completed deletion into an error; allocated space is reused
kept:           source rows and offsets, plus the monotonic retention cutoff. Replaced,
                truncated, copied and previously unread transcripts cannot restore older events
result:         {"ok": true, "schemaVersion": 1, "removed": N}; on a store failure ok false,
                removed 0, error "usage store unavailable", exit status 1
audit:          the backend's helper-write audit line records provider, helper and method only
```

Forget does not ingest first. The persisted cutoff also excludes unread history
and is checked atomically by concurrent writers. `removed` counts stored rows
deleted, not unread transcript records. Supplying an earlier cutoff later never
restores history. Deleting the database itself resets this retention policy.
Secure deletion does not erase a reader's existing WAL snapshot, filesystem
snapshots, storage blocks or backups.

## In the blades

`ui/ArtifactInventory.qml` has an opt-in history request. The Skills and MCP
providers set `activityMethod: "usage"`; Skills passes `["--json", "--project",
anchorPath]` plus its project arguments, MCP passes `["--json"]`. A request is
queued on `refresh()` (which a finished write also triggers) and on an anchor
change only while at least one heatmap is loaded. It starts after 180 ms and
runs one at a time. A pending answer schedules another request after 500 ms;
completion refreshes the inventory counts. Closing, hiding or shortening all
activity views stops the requests; ordinary inventory counts still refresh. A write or an
anchor change cancels the running request, and a stale generation's answer is
dropped. An anchor change clears the previous activity. A failed answer keeps
the last good payload in `activity`, sets `activityError` and shows the error
in the module header. Pending history displays “Reading activity…”.

`ui/UsageHeatmap.qml` takes that payload, a `calendarRule` for the locale's
first day of the week, and a `unitLabel` ("skill uses" or "MCP calls"). Cells
are `Style.space(8)` with a `Style.space(2)` gap; weeks are
`min(160, floor((width + gap) / pitch))`. Colour levels split at the 25th, 50th
and 75th percentile of the non-zero `uses` across the whole payload, so
widening a blade never recolours a cell. Days before `coverageStart`, or every
day when it is null, have no fill. The modules load it under the search field
only while the tab is open, the tab's `activity` view state is on (the header's
Activity button, default on) and the module is at least `Style.space(300)`
tall. Tab from search explicitly focuses the grid; Tab or Escape from the
grid focuses the tree, and Shift-Tab reveals and focuses search. Hiding a
focused grid returns focus to the tree.

The MCP module makes a definition with a non-empty `observed` list expandable.
`ArtifactTree.expansionKey` keys that expansion by the definition `id`, never
by the configuration path, because several definitions share one file. Each
observed entry becomes a leaf child with a kind glyph, its `uses` and `failed`
as metrics, and no actions. Right or `l` expands a definition, then moves to
its first child; physical folder navigation keeps its existing behavior.

User-visible behaviour is listed in section 48 of the
[UI expectations](../../tests/EXPECTATIONS.md).

## The CLI

```text
fileblade usage skills                           helper-read fileblade.core.skills usage
                                                 --project <current directory> --json
fileblade usage mcp                              helper-read fileblade.core.mcp usage --json
fileblade usage forget [--before YYYY-MM-DD]     helper-write fileblade.core.mcp usage-forget
```

```yaml
text:       one "YYYY-MM-DD<TAB>uses" line per day, nothing when there are no days;
            forget prints "removed N"
json:       the global -o json / --output json, before or after the subcommand, prints the
            helper document unchanged. There is no subcommand --json
--before:   must be a real zero-padded calendar date; 2026-9-01, 2026-02-30, 20260901 and
            +2026-09-01 exit 2 before the helper runs
errors:     a helper answer with ok false exits 1 with the helper's error
shell:      not needed. The CLI dispatches the backend request in its own process, and the
            helper runs under the app root with the caller's environment
```

## Known limits

```yaml
codex skills:        Codex writes no skill-use event; its model reads SKILL.md through shell
                     commands. Codex skill counts would be inference, so none are recorded
mcp prompts:         only the spelling <command-name>/mcp__<server>__<prompt></command-name> is
                     counted, verified on Claude Code 2.1.258 by typing /probe:greet (MCP). If a
                     later release records prompts differently they stop counting, although
                     the raw command events are still stored for a corrected rule to classify
transcript format:   Claude Code and Codex transcripts are undocumented internal formats and
                     can change in any release. Record types, field names and the command-name
                     tag are assumptions checked against 2.1.258 and local rollouts
unobserved records:  Claude resource tool records come from the 2.1.258 binary's schemas and one
                     probe session, not from everyday history. Codex failure detection has only
                     seen completed calls
plugin mcp servers:  redacted server names or plugin identities report 0 rather than guessing
first read:          history fills progressively, newest transcripts first. The CLI reports
                     ingestPending; visible heatmaps continue until ingestion finishes
walk cap:            beyond 8192 transcripts per agent, which ones are read follows directory
                     walk order, not age
native install:      with FILEBLADE_NATIVE_STATE_ROOT set, fileblade usage forget is refused with
                     "native owner-unavailable", like other CLI mutations; skills and mcp work
erasure:             secure_delete overwrites SQLite cells, not filesystem blocks or backups.
                     Existing readers can retain deleted pages in their WAL snapshot
```
