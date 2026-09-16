from __future__ import annotations

import json
import re
import sqlite3
from collections import Counter
from typing import Any

from .store import session

SCHEMA_VERSION = 1
MAX_OBSERVED = 64
WINDOW_DAYS = 160 * 7
UNAVAILABLE = "usage store unavailable"
MCP_EVENTS = "(kind IN ('tool', 'resource', 'resource-list') OR (agent = 'claude' AND kind = 'command' AND name GLOB 'mcp__?*__?*'))"
PLUGIN_CACHE = re.compile(r"/plugins/cache/[^/]+/([^/]+)/")


def sanitize(name: str) -> str:
    value = re.sub(r"[^a-zA-Z0-9_-]", "_", name)
    return re.sub(r"_+", "_", value).strip("_") if name.startswith("claude.ai ") else value


def skill_names(item: dict[str, Any]) -> set[str]:
    name = str(item.get("name") or "")
    source = str(item.get("source") or "")
    return {name, source[len("plugin:"):].split("@", 1)[0] + ":" + name} if source.startswith("plugin:") else {name}


def mcp_server(item: dict[str, Any]) -> tuple[str, str] | None:
    name = str(item.get("name") or "")
    if item.get("agent") == "codex":
        return "codex", name
    if item.get("agent") != "claude":
        return None
    if item.get("scope") != "plugin":
        return "claude", sanitize(name)
    plugin = PLUGIN_CACHE.search(str((item.get("source") or {}).get("path") or ""))
    return ("claude", f"plugin_{sanitize(plugin.group(1))}_{sanitize(name)}") if plugin else None


def counts(agent: int, user: int, scheduled: int, failed: int) -> dict[str, int]:
    return {"uses": agent + user, "usesAgent": agent, "usesUser": user, "usesScheduled": scheduled, "failed": failed}


def attach(item: dict[str, Any], values: dict[str, int]) -> None:
    item.update(values)
    item.setdefault("metrics", {}).update(values)


def sources(connection: sqlite3.Connection, agents: tuple[str, ...]) -> int:
    return connection.execute("SELECT count(*) FROM source WHERE agent IN (SELECT value FROM json_each(?))",
                              (json.dumps(agents),)).fetchone()[0]


def attach_skills(items: list[dict[str, Any]]) -> dict[str, Any]:
    try:
        with session() as (connection, (pending, unreadable)):
            tallies: dict[str, list[int]] = {}
            for kind, name, origin, total, failed in connection.execute(
                    "SELECT kind, name, origin, count(*), sum(failed) FROM event WHERE kind IN ('skill', 'command') "
                    "GROUP BY kind, name, origin"):
                tally = tallies.setdefault(name, [0, 0, 0, 0])
                tally[0 if kind == "skill" else 1 if origin == "user" else 2] += total
                tally[3] += failed if kind == "skill" else 0
            for item in items:
                attach(item, counts(*(sum(column) for column in zip(*(tallies.get(name, [0, 0, 0, 0]) for name in skill_names(item))))))
            return {"usageTranscripts": sources(connection, ("claude",)), "usageUnreadable": unreadable, "usageIngestPending": pending}
    except (OSError, sqlite3.Error):
        return {"usageError": UNAVAILABLE}


def attach_mcp(definitions: list[dict[str, Any]]) -> dict[str, Any]:
    try:
        with session() as (connection, (pending, unreadable)):
            servers: dict[tuple[str, str], dict[tuple[str, str], list[Any]]] = {}
            for agent, server, kind, name, origin, total, failed, last in connection.execute(
                    "SELECT agent, server, kind, name, origin, count(*), sum(failed), "
                    f"date(max(at) / 1000, 'unixepoch', 'localtime') FROM event WHERE {MCP_EVENTS} "
                    "GROUP BY agent, server, kind, name, origin"):
                if kind == "command":
                    _, server, name = name.split("__", 2)
                    kind = "prompt"
                entry = servers.setdefault((agent, sanitize(server) if agent == "claude" else server), {}).setdefault((kind, name), [0, 0, 0, 0, ""])
                entry[0 if kind != "prompt" else 1 if origin == "user" else 2] += total
                entry[3] += failed
                entry[4] = max(entry[4], last)
            keys = [mcp_server(item) for item in definitions]
            owners = Counter(key for key in keys if key)
            ambiguous = 0
            for item, key in zip(definitions, keys):
                observed = servers.get(key, {}) if key else {}
                if key and owners[key] > 1:
                    ambiguous += 1
                    item["usageAmbiguous"] = True
                    observed = {}
                attach(item, counts(*(sum(entry[slot] for entry in observed.values()) for slot in range(4))))
                ranked = sorted(observed.items(), key=lambda pair: (-pair[1][0] - pair[1][1], pair[0][1], pair[0][0]))
                item["observed"] = [{"kind": kind, "name": name, "uses": entry[0] + entry[1], "failed": entry[3], "lastUsed": entry[4]}
                                    for (kind, name), entry in ranked[:MAX_OBSERVED]]
            return {"usageTranscripts": sources(connection, ("claude", "codex")), "usageUnreadable": unreadable,
                    "usageIngestPending": pending, "usageAmbiguous": ambiguous}
    except (OSError, sqlite3.Error):
        return {"usageError": UNAVAILABLE}


def history(kind: str, agents: tuple[str, ...], where: str, parameters: tuple[Any, ...] = ()) -> dict[str, Any]:
    try:
        with session() as (connection, (pending, _)):
            start, until = connection.execute(
                "SELECT date(min(first_at) / 1000, 'unixepoch', 'localtime'), date('now', 'localtime') FROM coverage "
                "WHERE agent IN (SELECT value FROM json_each(?))", (json.dumps(agents),)).fetchone()
            days = connection.execute(
                "SELECT date(at / 1000, 'unixepoch', 'localtime') AS day, sum(kind <> 'command' OR origin = 'user'), "
                "sum(kind <> 'command'), sum(kind = 'command' AND origin = 'user'), "
                "sum(kind = 'command' AND origin = 'scheduled'), sum(failed) "
                f"FROM event WHERE {where} AND day > date('now', 'localtime', '-{WINDOW_DAYS} days') "
                "AND day <= date('now', 'localtime') GROUP BY day ORDER BY day", parameters).fetchall()
            return {"ok": True, "schemaVersion": SCHEMA_VERSION, "kind": kind, "coverageStart": start, "until": until,
                    "ingestPending": pending, "days": [list(day) for day in days]}
    except (OSError, sqlite3.Error):
        return {"ok": False, "schemaVersion": SCHEMA_VERSION, "kind": kind, "error": UNAVAILABLE}


def skill_usage(items: list[dict[str, Any]]) -> dict[str, Any]:
    names = sorted({name for item in items for name in skill_names(item)})
    return history("skill", ("claude",),
                   "(kind = 'skill' OR (kind = 'command' AND name IN (SELECT value FROM json_each(?))))", (json.dumps(names),))


def mcp_usage() -> dict[str, Any]:
    return history("mcp", ("claude", "codex"), MCP_EVENTS)


def forget(before: str | None) -> dict[str, Any]:
    try:
        with session() as (connection, _):
            with connection:
                connection.execute("BEGIN IMMEDIATE")
                if before:
                    cutoff = connection.execute("SELECT CAST(strftime('%s', ?, 'utc') AS INTEGER) * 1000", (before,)).fetchone()[0]
                    removed = connection.execute("DELETE FROM event WHERE at < ?", (cutoff,)).rowcount
                    connection.execute("UPDATE coverage SET first_at = max(first_at, ?)", (cutoff,))
                else:
                    removed = connection.execute("DELETE FROM event").rowcount
                    for statement in ("DELETE FROM coverage", "UPDATE source SET project = NULL", "DELETE FROM project"):
                        connection.execute(statement)
            connection.execute("VACUUM")
            return {"ok": True, "schemaVersion": SCHEMA_VERSION, "removed": removed}
    except (OSError, sqlite3.Error):
        return {"ok": False, "schemaVersion": SCHEMA_VERSION, "removed": 0, "error": UNAVAILABLE}
