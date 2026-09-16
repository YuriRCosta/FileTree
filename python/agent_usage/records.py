from __future__ import annotations

import re
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from typing import Any

COMMAND = re.compile(r"<command-name>/([A-Za-z0-9:_-]+)</command-name>")
QUERY = re.compile(r"[?#]")
EPOCH = datetime(1970, 1, 1, tzinfo=timezone.utc)
MAX_URI_BYTES = 512


@dataclass
class Batch:
    offset: int
    project: str | None
    first_at: int | None = None
    events: list[tuple[Any, ...]] = field(default_factory=list)
    failures: list[tuple[str]] = field(default_factory=list)

    def stamp(self, record: dict[str, Any]) -> int | None:
        try:
            moment = datetime.fromisoformat(text(record.get("timestamp")))
        except ValueError:
            return None
        at = (moment.replace(tzinfo=moment.tzinfo or timezone.utc) - EPOCH) // timedelta(milliseconds=1)
        self.first_at = at if self.first_at is None else min(self.first_at, at)
        return at


def text(value: Any) -> str:
    return value if isinstance(value, str) else ""


def mapping(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


def tool(part: dict[str, Any]) -> tuple[str, str, str] | None:
    name = text(part.get("name"))
    arguments = mapping(part.get("input"))
    if name == "Skill":
        skill = text(arguments.get("skill"))
        return ("skill", "", skill) if skill else None
    if name == "ListMcpResourcesTool":
        return "resource-list", text(arguments.get("server")), ""
    if name in ("ReadMcpResourceTool", "ReadMcpResourceDirTool"):
        uri = QUERY.split(text(arguments.get("uri")), maxsplit=1)[0]
        return "resource", text(arguments.get("server")), uri.encode("utf-8", "replace")[:MAX_URI_BYTES].decode("utf-8", "ignore")
    segments = name.split("__")
    if segments[0] == "mcp" and len(segments) >= 3 and segments[1]:
        return "tool", segments[1], "__".join(segments[2:])
    return None


def claude_line(raw: bytes) -> bool:
    return b'"Skill"' in raw or b"mcp__" in raw or b"McpResource" in raw or b"command-name" in raw or b'"is_error":true' in raw


def codex_line(raw: bytes) -> bool:
    return b"McpToolCall" in raw


def claude(record: dict[str, Any], batch: Batch) -> None:
    at = batch.stamp(record)
    content = mapping(record.get("message")).get("content")
    project = record.get("cwd") if isinstance(record.get("cwd"), str) else None
    subagent = int(record.get("isSidechain") is True)
    if record.get("type") == "assistant" and isinstance(content, list) and at is not None:
        for part in content:
            if not isinstance(part, dict) or part.get("type") != "tool_use" or not text(part.get("id")):
                continue
            event = tool(part)
            if event:
                kind, server, name = event
                batch.events.append(("claude", part["id"], at, kind, "agent", server, name, subagent, project, 0))
    if record.get("type") != "user":
        return
    texts = [content] if isinstance(content, str) else []
    for part in content if isinstance(content, list) else []:
        if not isinstance(part, dict):
            continue
        if part.get("type") == "tool_result" and part.get("is_error") is True and text(part.get("tool_use_id")):
            batch.failures.append((part["tool_use_id"],))
        elif part.get("type") == "text":
            texts.append(text(part.get("text")))
    names = COMMAND.findall("\n".join(texts))
    uuid = text(record.get("uuid"))
    if not names or not uuid or at is None:
        return
    origin = "scheduled" if record.get("scheduledTaskId") else "user"
    for index, name in enumerate(names, 1):
        call = uuid if len(names) == 1 else f"{uuid}:{index}"
        batch.events.append(("claude", call, at, "command", origin, "", name, subagent, project, 0))


def codex(record: dict[str, Any], batch: Batch) -> None:
    at = batch.stamp(record)
    payload = mapping(record.get("payload"))
    if record.get("type") == "session_meta" and isinstance(payload.get("cwd"), str):
        batch.project = payload["cwd"]
    item = mapping(payload.get("item"))
    if (record.get("type") != "event_msg" or payload.get("type") != "item_completed" or item.get("type") != "McpToolCall"
            or at is None or not text(item.get("id")) or not text(item.get("server")) or not text(item.get("tool"))):
        return
    failed = item.get("status") != "completed" or mapping(item.get("result")).get("isError") is True
    batch.events.append(("codex", item["id"], at, "tool", "agent", item["server"], item["tool"], 0, batch.project, int(failed)))
