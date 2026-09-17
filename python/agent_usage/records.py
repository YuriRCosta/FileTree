from __future__ import annotations

import posixpath
import re
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from typing import Any

COMMAND = re.compile(r"<command-name>/([A-Za-z0-9:_-]+)</command-name>")
SLASH = re.compile(r"^/([A-Za-z0-9:_.-]+)")
QUERY = re.compile(r"[?#]")
EPOCH = datetime(1970, 1, 1, tzinfo=timezone.utc)
MAX_URI_BYTES = 512
SKILL_FILE = "SKILL.md"
COPILOT_SKIPPED_TRIGGERS = ("context-load",)
FINISHED = ("completed", "error")


@dataclass
class Batch:
    offset: int
    project: str | None
    source: str = ""
    pending: bool = False
    first_at: int | None = None
    events: list[tuple[Any, ...]] = field(default_factory=list)
    failures: list[tuple[str, int]] = field(default_factory=list)

    def stamp(self, record: dict[str, Any], key: str = "timestamp") -> int | None:
        value = record.get(key)
        if isinstance(value, (int, float)) and not isinstance(value, bool):
            at = int(value)
        else:
            try:
                moment = datetime.fromisoformat(text(value))
            except ValueError:
                return None
            at = (moment.replace(tzinfo=moment.tzinfo or timezone.utc) - EPOCH) // timedelta(milliseconds=1)
        return self.note(at)

    def note(self, at: int) -> int:
        self.first_at = at if self.first_at is None else min(self.first_at, at)
        return at


def text(value: Any) -> str:
    return value if isinstance(value, str) else ""


def mapping(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


def skill_from_path(path: str) -> str:
    normalized = posixpath.normpath(text(path))
    if posixpath.basename(normalized) != SKILL_FILE:
        return ""
    return posixpath.basename(posixpath.dirname(normalized))


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
    return b'"Skill"' in raw or b"mcp__" in raw or b"McpResource" in raw or b"command-name" in raw or b'"is_error"' in raw


def codex_line(raw: bytes) -> bool:
    return (b"McpToolCall" in raw or b"SKILL.md" in raw or b"selected_skill_instructions" in raw or b"<skill>" in raw
            or b"session_meta" in raw or b'"namespace"' in raw)


def copilot_line(raw: bytes) -> bool:
    return b"skill.invoked" in raw or b"tool.execution_" in raw or b"session.start" in raw or b"session.context_changed" in raw


def antigravity_line(raw: bytes) -> bool:
    return b"SKILL.md" in raw or b"slash_command" in raw


def pi_line(raw: bytes) -> bool:
    return b"SKILL.md" in raw or b"toolResult" in raw or b'"session"' in raw


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
        if part.get("type") == "tool_result" and part.get("is_error") is True and text(part.get("tool_use_id")) and at is not None:
            batch.failures.append((part["tool_use_id"], at))
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


CODEX_SKILL_KIND = "skills.selected_skill_instructions"
CODEX_SKILL_NAME = re.compile(r"<skill>\s*<name>([^<\n]+)</name>")
SKILL_PATH_WORD = re.compile(r"(?<![\w./-])((?:/|~/)[^\s'\"`;|&<>()]*?/SKILL\.md)")


def skills_in_command(command: str) -> list[str]:
    found: list[str] = []
    for match in SKILL_PATH_WORD.findall(text(command)):
        skill = skill_from_path(match)
        if skill and skill not in found:
            found.append(skill)
    return found


def codex_explicit(payload: dict[str, Any], at: int, batch: Batch) -> None:
    passthrough = mapping(payload.get("internal_chat_message_metadata_passthrough"))
    kinds = passthrough.get("content_item_kinds") if isinstance(passthrough.get("content_item_kinds"), list) else []
    content = payload.get("content") if isinstance(payload.get("content"), list) else []
    identity = text(payload.get("id")) or text(passthrough.get("turn_id"))
    if not identity:
        return
    for index, part in enumerate(content):
        body = text(mapping(part).get("text"))
        tagged = index < len(kinds) and kinds[index] == CODEX_SKILL_KIND
        match = CODEX_SKILL_NAME.search(body) if tagged or body.lstrip().startswith("<skill>") else None
        if match:
            batch.events.append(("codex", f"{identity}:{index}", at, "command", "user", "", match.group(1).strip(), 0, batch.project, 0))


def codex_item(payload: dict[str, Any], at: int, batch: Batch) -> None:
    item = mapping(payload.get("item"))
    identity = text(item.get("id"))
    if not identity:
        return
    if item.get("type") == "McpToolCall" and text(item.get("server")) and text(item.get("tool")):
        failed = item.get("status") != "completed" or mapping(item.get("result")).get("isError") is True
        batch.events.append(("codex", identity, at, "tool", "agent", item["server"], item["tool"], 0, batch.project, int(failed)))
        return
    if item.get("type") != "CommandExecution":
        return
    parsed = item.get("parsed_cmd") if isinstance(item.get("parsed_cmd"), list) else []
    skills = [skill_from_path(text(mapping(entry).get("path"))) for entry in parsed if mapping(entry).get("type") == "read"]
    skills = [skill for skill in skills if skill]
    if not skills:
        command = item.get("command")
        skills = skills_in_command(" ".join(command) if isinstance(command, list) else text(command))
    turn = text(payload.get("turn_id"))
    failed = int(text(item.get("status")) not in ("", "completed"))
    for skill in dict.fromkeys(skills):
        call = f"{turn}:{skill}" if turn else f"{identity}:{skill}"
        batch.events.append(("codex", call, at, "skill", "agent", "", skill, 0, batch.project, failed))


def codex_legacy(payload: dict[str, Any], at: int, batch: Batch) -> None:
    call = text(payload.get("call_id")) or text(payload.get("id"))
    if not call:
        return
    if payload.get("type") == "custom_tool_call" and text(payload.get("name")) == "exec":
        for skill in skills_in_command(text(payload.get("input"))):
            batch.events.append(("codex", f"{call}:{skill}", at, "skill", "agent", "", skill, 0, batch.project, 0))
    elif payload.get("type") == "function_call" and text(payload.get("namespace")) and text(payload.get("name")):
        batch.events.append(("codex", call, at, "tool", "agent", payload["namespace"], payload["name"], 0, batch.project, 0))


def codex(record: dict[str, Any], batch: Batch) -> None:
    at = batch.stamp(record)
    payload = mapping(record.get("payload"))
    kind = record.get("type")
    if kind == "session_meta" and isinstance(payload.get("cwd"), str):
        batch.project = payload["cwd"]
    if at is None:
        return
    if kind == "event_msg" and payload.get("type") == "item_completed":
        codex_item(payload, at, batch)
    elif kind == "response_item" and payload.get("type") == "message" and payload.get("role") == "user":
        codex_explicit(payload, at, batch)
    elif kind == "response_item":
        codex_legacy(payload, at, batch)


def copilot(record: dict[str, Any], batch: Batch) -> None:
    at = batch.stamp(record)
    kind = text(record.get("type"))
    data = mapping(record.get("data"))
    identity = text(record.get("id"))
    subagent = int(bool(text(record.get("agentId"))))
    if kind in ("session.start", "session.context_changed"):
        cwd = text(mapping(data.get("context")).get("cwd")) or text(data.get("cwd"))
        if cwd:
            batch.project = cwd
        return
    if at is None or not identity:
        return
    if kind == "skill.invoked":
        name = text(data.get("name"))
        trigger = text(data.get("trigger"))
        if not name or trigger in COPILOT_SKIPPED_TRIGGERS:
            return
        if trigger == "user-invoked":
            batch.events.append(("copilot", identity, at, "command", "user", "", name, subagent, batch.project, 0))
        else:
            batch.events.append(("copilot", identity, at, "skill", "agent", "", name, subagent, batch.project, 0))
        return
    call = text(data.get("toolCallId"))
    if kind == "tool.execution_start" and call:
        server = text(data.get("mcpConfigServerName")) or text(data.get("mcpServerName"))
        if not server:
            return
        name = text(data.get("mcpToolName")) or text(data.get("toolName"))
        batch.events.append(("copilot", call, at, "tool", "agent", server, name, subagent, batch.project, 0))
    elif kind == "tool.execution_complete" and call and data.get("success") is False:
        batch.failures.append((call, at))


def antigravity(record: dict[str, Any], batch: Batch) -> None:
    if "display" in record and "timestamp" in record:
        at = batch.stamp(record)
        if at is None or record.get("type") != "slash_command":
            return
        match = SLASH.match(text(record.get("display")).strip())
        if not match:
            return
        project = text(record.get("workspace")) or None
        call = f"{text(record.get('conversationId')) or text(record.get('workspace'))}:{at}"
        batch.events.append(("antigravity", call, at, "command", "user", "", match.group(1), 0, project, 0))
        return
    at = batch.stamp(record, "created_at")
    calls = record.get("tool_calls")
    step = record.get("step_index")
    if at is None or not isinstance(calls, list) or not isinstance(step, int) or isinstance(step, bool):
        return
    failed = int(text(record.get("status")).upper() == "ERROR")
    for index, call in enumerate(calls):
        if not isinstance(call, dict) or text(call.get("name")) != "view_file":
            continue
        skill = skill_from_path(text(mapping(call.get("args")).get("AbsolutePath")))
        if skill:
            batch.events.append(("antigravity", f"{batch.source}:{step}:{index}", at, "skill", "agent", "", skill, 0, batch.project, failed))


def pi(record: dict[str, Any], batch: Batch) -> None:
    if record.get("type") == "session":
        batch.stamp(record)
        if isinstance(record.get("cwd"), str):
            batch.project = record["cwd"]
        return
    if record.get("type") != "message":
        return
    at = batch.stamp(record)
    message = mapping(record.get("message"))
    if at is None:
        return
    if message.get("role") == "toolResult":
        call = text(message.get("toolCallId"))
        if call and message.get("isError") is True:
            batch.failures.append((call, at))
        return
    if message.get("role") != "assistant":
        return
    for part in message.get("content") if isinstance(message.get("content"), list) else []:
        if not isinstance(part, dict) or part.get("type") != "toolCall" or not text(part.get("id")):
            continue
        arguments = mapping(part.get("arguments"))
        skill = skill_from_path(text(arguments.get("path")) or text(arguments.get("file_path")))
        if not skill and text(part.get("name")) == "bash":
            skill = next((skill_from_path(word) for word in text(arguments.get("command")).split() if word.endswith(SKILL_FILE)), "")
        if skill:
            batch.events.append(("pi", part["id"], at, "skill", "agent", "", skill, 0, batch.project, 0))


def opencode_part(data: dict[str, Any], call_default: str, at: int, project: str | None, batch: Batch) -> None:
    state = mapping(data.get("state"))
    status = text(state.get("status"))
    if data.get("type") != "tool" or status not in FINISHED:
        return
    call = text(data.get("callID")) or call_default
    name = text(data.get("tool")) or text(data.get("name"))
    if not call or not name:
        return
    failed = int(status == "error")
    if name == "skill":
        skill = text(mapping(state.get("input")).get("name"))
        if skill:
            batch.events.append(("opencode", call, at, "skill", "agent", "", skill, 0, project, failed))
        return
    if name in OPENCODE_BUILTIN_TOOLS or "_" not in name:
        return
    batch.events.append(("opencode", call, at, "tool", "agent", "", name, 0, project, failed))


OPENCODE_BUILTIN_TOOLS = frozenset({
    "bash", "shell", "read", "write", "edit", "multiedit", "patch", "glob", "grep", "list", "ls", "webfetch", "websearch",
    "todowrite", "todoread", "task", "question", "skill", "lsp", "codesearch", "apply_patch", "web_fetch", "web_search",
    "todo_write", "todo_read", "batch", "invalid",
})

LINE_READERS = {
    "claude": (claude_line, claude),
    "codex": (codex_line, codex),
    "copilot": (copilot_line, copilot),
    "antigravity": (antigravity_line, antigravity),
    "pi": (pi_line, pi),
}
