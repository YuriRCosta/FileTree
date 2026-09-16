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
version:      PRAGMA user_version = 1. Any other value drops every table and recreates the
              schema, and history rebuilds from the transcripts that still exist
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
```

```yaml
source:    one row per transcript file: identity, size, mtime in nanoseconds, and the byte
           offset read so far. Codex rollouts also carry their session's project
project:   working directories seen in transcripts
coverage:  per agent, the earliest record timestamp ever read. Kept as the minimum across
           runs, so deleting old transcripts never moves it later
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
failure:        user tool_result with is_error true marks the matching claude event failed
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
that touches usage (`list`, `usage` and `usage-forget`) first runs one
budgeted ingest in its own process, then answers from what is committed. A
blade refresh runs `list` for its project and user lanes and one `usage`
request, so new transcript bytes arrive with the next refresh of an open
Skills or MCP tab, after a mutation, or when the tab's anchor folder changes.

```yaml
roots:        claude: $CLAUDE_CONFIG_DIR/projects, else ~/.claude/projects, every *.jsonl
              below, subagent transcripts included
              codex: $CODEX_HOME/sessions and $CODEX_HOME/archived_sessions, else ~/.codex/...
walk cap:     8192 regular files per agent, taken in directory walk order
order:        newest mtime first
lock:         non-blocking flock on the lock file, retried every 50 ms for up to 4 s. A helper
              that never gets it skips ingest and reports pending
budget:       3 s from the start of ingest, lock wait included. Checked between files: once it
              is spent no new file starts and the answer reports pending
unchanged:    a file whose device, inode, size and mtime match its source row is skipped
offsets:      reading resumes at the stored offset. A changed device or inode, or a size below
              the offset, restarts the file at byte 0
partial line: reading stops at the first line without a trailing newline, so a half-written
              record waits for the next call
record cap:   lines over 4 MiB are skipped
prefilter:    Claude lines are parsed only when they contain "Skill", mcp__, McpResource,
              command-name or "is_error":true; Codex lines only when they contain
              McpToolCall. On a read from byte 0, lines carrying "timestamp" are also parsed
              until the first record with a real timestamp, so coverage starts where the
              file does
transaction:  one BEGIN IMMEDIATE per file: INSERT OR IGNORE events, failure updates, the
              coverage minimum and the source row with its new offset commit together. A crash
              or timeout never double counts or skips bytes
duplicates:   the (agent, call) key makes re-reading idempotent and drops a command copied into
              a resumed session
unreadable:   files that cannot be stat'ed or opened are counted and reported, never fatal
vanished:     after a scan that finished within budget, source rows whose file no longer exists
              are deleted. Events are never deleted by ingest
```

Measured on one host with 997 transcripts (6.5 GiB, 5.3 GiB of it Codex): a
cold store needs about 5 s over two helper calls, the first stopping at the
budget with `ingestPending` true; a warm call takes 0.06 to 0.09 s per helper
process. The store was 700 KB.

## Name matching

Matching lives in `query.py` and is shared by `list` and `usage`.

```yaml
sanitize(name):   re.sub(r"[^a-zA-Z0-9_-]", "_", name); for a name starting with
                  "claude.ai " also collapse runs of "_" and strip them from both ends.
                  This is the rule Claude Code 2.1.258 uses to build mcp__ tool names
skill row:        events of kind skill or command named exactly the row name, or
                  <plugin>:<row name> when the row source is plugin:<plugin>@<marketplace>
mcp, claude:      the row's event server is sanitize(name). A plugin-scope row uses
                  plugin_<sanitize(plugin)>_<sanitize(name)>, where plugin is the directory
                  after the marketplace in ~/.claude/plugins/cache/<marketplace>/<plugin>/...
                  Stored Claude servers are sanitized before comparison, so a resource call
                  recorded with input.server "my.server" or "plugin:toolkit:docs" meets the
                  same row as mcp__my_server__ or mcp__plugin_toolkit_docs__ tool calls
mcp, codex:       the event server equals the configured name
mcp, other:       definitions of any other agent never match and report 0
ambiguity:        two or more definitions of one agent resolving to the same event server all
                  report 0, carry usageAmbiguous true and list no observed entries
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
with --before:  runs the normal budgeted ingest, then deletes events before local midnight at
                the start of that day and raises every coverage.first_at to that moment
without:        runs the ingest, then deletes every event, every coverage row and every project
                row (source.project is cleared first)
then:           VACUUM, so removed rows do not linger in free pages of the database file
kept:           source rows and offsets, so transcripts already read are not imported again
result:         {"ok": true, "schemaVersion": 1, "removed": N}; on a store failure ok false,
                removed 0, error "usage store unavailable", exit status 1
audit:          the backend's helper-write audit line records provider, helper and method only
```

Ingesting first matters: unread bytes of transcripts that already exist would
otherwise bring the forgotten history back on the next call.

## In the blades

`ui/ArtifactInventory.qml` has an opt-in history request. The Skills and MCP
providers set `activityMethod: "usage"`; Skills passes `["--json", "--project",
anchorPath]` plus its project arguments, MCP passes `["--json"]`. A request is
queued on every `refresh()` (which a finished write also triggers) and on an
anchor change, starts after 180 ms, and runs one at a time. A write or an
anchor change cancels the running request, and a stale generation's answer is
dropped. A failed answer keeps the last good payload in `activity` and sets
`activityError`.

`ui/UsageHeatmap.qml` takes that payload, a `calendarRule` for the locale's
first day of the week, and a `unitLabel` ("skill uses" or "MCP calls"). Cells
are `Style.space(8)` with a `Style.space(2)` gap; weeks are
`min(160, floor((width + gap) / pitch))`. Colour levels split at the 25th, 50th
and 75th percentile of the non-zero `uses` across the whole payload, so
widening a blade never recolours a cell. Days before `coverageStart`, or every
day when it is null, have no fill. The modules load it under the search field
only while the tab is open, the tab's `activity` view state is on (the header's
Activity button, default on) and the module is at least `Style.space(300)`
tall. Its `dismissed()` signal returns focus to the tree.

The MCP module makes a definition with a non-empty `observed` list expandable.
`ArtifactTree.expansionKey` keys that expansion by the definition `id`, never
by the configuration path, because several definitions share one file. Each
observed entry becomes a leaf child with a kind glyph, its `uses` and `failed`
as metrics, and no actions.

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
                     can change in any release. Record types, field names, the <command-name>
                     tag and compact JSON (the prefilter looks for "is_error":true without a
                     space) are all assumptions checked against 2.1.258 and local rollouts
unobserved records:  Claude resource tool records come from the 2.1.258 binary's schemas and one
                     probe session, not from everyday history. Codex failure detection has only
                     seen completed calls
plugin mcp servers:  matching needs the plugin cache path /plugins/cache/<marketplace>/<plugin>/.
                     A plugin installed elsewhere, or a path or name the inventory redacted,
                     reports 0
first read:          a large history takes more than one call. The first answer can be partial,
                     newest transcripts first, and the blades do not ask again on
                     ingestPending; the rest arrives with later refreshes
budget granularity:  the budget is checked between files. A single transcript that takes longer
                     than the 8 s helper timeout to read never commits and restarts every call
walk cap:            beyond 8192 transcripts per agent, which ones are read follows directory
                     walk order, not age
forget and rewrites: a transcript whose inode changes or that shrinks is read again from byte
                     0, and its forgotten events come back
forget and VACUUM:   the delete commits before VACUUM. A VACUUM that stays busy past 5 s reports
                     "usage store unavailable" although the events are already gone
native install:      with FILEBLADE_NATIVE_STATE_ROOT set, fileblade usage forget is refused with
                     "native owner-unavailable", like other CLI mutations; skills and mcp work
hidden heatmap:      the history request runs on every refresh even while Activity is off, and
                     activityError is not shown in either blade
erasure:             VACUUM is not secure deletion. Old pages can remain in the WAL until the
                     last connection closes, in filesystem blocks and in backups
```
