from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable

SCHEMA = 1
COMMAND = re.compile(r"<command-name>/([A-Za-z0-9:_-]+)</command-name>")
MAX_RECORD_BYTES = 4 * 1024 * 1024
MAX_FILES = 8192


@dataclass
class Tally:
    agent: int = 0
    user: int = 0
    scheduled: int = 0
    failed: int = 0

    def merge(self, other: "Tally") -> None:
        self.agent += other.agent
        self.user += other.user
        self.scheduled += other.scheduled
        self.failed += other.failed

    def document(self) -> dict[str, int]:
        return {
            "uses": self.agent + self.user,
            "usesAgent": self.agent,
            "usesUser": self.user,
            "usesScheduled": self.scheduled,
            "failed": self.failed,
        }


@dataclass
class FileTally:
    skills: dict[str, Tally] = field(default_factory=dict)
    servers: dict[str, Tally] = field(default_factory=dict)
    pending: dict[str, list[str]] = field(default_factory=dict)
    offset: int = 0
    size: int = 0
    mtime: int = 0
    inode: int = 0
    device: int = 0

    def skill(self, name: str) -> Tally:
        return self.skills.setdefault(name, Tally())

    def server(self, name: str) -> Tally:
        return self.servers.setdefault(name, Tally())


def transcript_roots() -> list[Path]:
    home = Path(os.environ.get("CLAUDE_CONFIG_DIR") or (Path.home() / ".claude"))
    return [home / "projects"]


def cache_path() -> Path:
    base = os.environ.get("XDG_CACHE_HOME") or (Path.home() / ".cache")
    return Path(base) / "omarchy" / "fileblade" / "agent-usage.json"


def transcripts(roots: Iterable[Path]) -> list[Path]:
    found: list[Path] = []
    for root in roots:
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.jsonl")):
            found.append(path)
            if len(found) >= MAX_FILES:
                return found
    return found


def _text_of(content: Any) -> str:
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return ""
    return " ".join(
        str(part.get("text") or "")
        for part in content
        if isinstance(part, dict) and part.get("type") == "text"
    )


def consume(record: dict[str, Any], tally: FileTally, known: set[str] | None) -> None:
    message = record.get("message")
    message = message if isinstance(message, dict) else {}
    content = message.get("content")
    if record.get("type") == "user":
        if isinstance(content, list):
            for part in content:
                if not isinstance(part, dict) or part.get("type") != "tool_result":
                    continue
                origin = tally.pending.pop(str(part.get("tool_use_id") or ""), None)
                if origin and part.get("is_error"):
                    kind, name = origin
                    target = tally.skill(name) if kind == "skill" else tally.server(name)
                    target.failed += 1
        for name in COMMAND.findall(_text_of(content)):
            if known is not None and name not in known:
                continue
            if record.get("scheduledTaskId"):
                tally.skill(name).scheduled += 1
            else:
                tally.skill(name).user += 1
        return
    if not isinstance(content, list):
        return
    for part in content:
        if not isinstance(part, dict) or part.get("type") != "tool_use":
            continue
        name = str(part.get("name") or "")
        identity = str(part.get("id") or "")
        if name == "Skill":
            skill = str((part.get("input") or {}).get("skill") or "")
            if skill:
                tally.skill(skill).agent += 1
                if identity:
                    tally.pending[identity] = ["skill", skill]
        elif name.startswith("mcp__"):
            segments = name.split("__")
            if len(segments) >= 3 and segments[1]:
                tally.server(segments[1]).agent += 1
                if identity:
                    tally.pending[identity] = ["server", segments[1]]


def read_file(path: Path, previous: FileTally | None, known: set[str] | None) -> tuple[FileTally, str]:
    try:
        stat = path.stat()
    except OSError as error:
        return FileTally(), f"{path}: {error.strerror or error}"
    reuse = (
        previous is not None
        and previous.inode == stat.st_ino
        and previous.device == stat.st_dev
        and previous.size <= stat.st_size
        and previous.offset <= stat.st_size
    )
    tally = previous if reuse else FileTally()
    if not reuse:
        tally = FileTally()
    start = tally.offset if reuse else 0
    if reuse and start == stat.st_size and previous is not None and previous.mtime == int(stat.st_mtime_ns):
        return tally, ""
    error = ""
    try:
        with path.open("rb") as handle:
            handle.seek(start)
            committed = start
            for raw in handle:
                if not raw.endswith(b"\n"):
                    break
                committed += len(raw)
                if len(raw) > MAX_RECORD_BYTES:
                    continue
                if b'"Skill"' not in raw and b"mcp__" not in raw and b"command-name" not in raw and b"tool_result" not in raw:
                    continue
                try:
                    record = json.loads(raw)
                except (ValueError, UnicodeDecodeError):
                    continue
                if isinstance(record, dict):
                    consume(record, tally, known)
            tally.offset = committed
    except OSError as as_error:
        error = f"{path}: {as_error.strerror or as_error}"
    tally.size = stat.st_size
    tally.mtime = int(stat.st_mtime_ns)
    tally.inode = stat.st_ino
    tally.device = stat.st_dev
    return tally, error


def server_aliases(name: str) -> list[str]:
    """A transcript names a server as the agent called it.

    Plugin-provided servers arrive as plugin_<plugin>_<server>, so the bare
    server name is offered as a second candidate.
    """
    candidates = [name]
    if name.startswith("plugin_"):
        stripped = name[len("plugin_"):]
        candidates.append(stripped)
        if "_" in stripped:
            candidates.append(stripped.split("_", 1)[1])
    return candidates


def load_cache(path: Path) -> dict[str, FileTally]:
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    if not isinstance(document, dict) or document.get("schema") != SCHEMA:
        return {}
    cached: dict[str, FileTally] = {}
    for name, raw in (document.get("files") or {}).items():
        if not isinstance(raw, dict):
            continue
        tally = FileTally(
            offset=int(raw.get("offset") or 0),
            size=int(raw.get("size") or 0),
            mtime=int(raw.get("mtime") or 0),
            inode=int(raw.get("inode") or 0),
            device=int(raw.get("device") or 0),
        )
        for group, target in (("skills", tally.skills), ("servers", tally.servers)):
            for key, counts in (raw.get(group) or {}).items():
                target[key] = Tally(
                    agent=int(counts.get("agent") or 0),
                    user=int(counts.get("user") or 0),
                    scheduled=int(counts.get("scheduled") or 0),
                    failed=int(counts.get("failed") or 0),
                )
        tally.pending = {k: list(v) for k, v in (raw.get("pending") or {}).items() if isinstance(v, list)}
        cached[name] = tally
    return cached


def save_cache(path: Path, files: dict[str, FileTally]) -> None:
    document = {"schema": SCHEMA, "files": {}}
    for name, tally in files.items():
        document["files"][name] = {
            "offset": tally.offset,
            "size": tally.size,
            "mtime": tally.mtime,
            "inode": tally.inode,
            "device": tally.device,
            "pending": tally.pending,
            "skills": {k: vars(v) for k, v in tally.skills.items()},
            "servers": {k: vars(v) for k, v in tally.servers.items()},
        }
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix(".tmp")
        temporary.write_text(json.dumps(document), encoding="utf-8")
        os.replace(temporary, path)
    except OSError:
        pass


def collect(known: set[str] | None = None, roots: Iterable[Path] | None = None,
            cache: Path | None = None, use_cache: bool = True) -> dict[str, Any]:
    roots = list(roots) if roots is not None else transcript_roots()
    store = cache if cache is not None else cache_path()
    cached = load_cache(store) if use_cache else {}
    skills: dict[str, Tally] = {}
    servers: dict[str, Tally] = {}
    files: dict[str, FileTally] = {}
    errors: list[str] = []
    paths = transcripts(roots)
    for path in paths:
        key = str(path)
        tally, error = read_file(path, cached.get(key), known)
        if error:
            errors.append(error)
        files[key] = tally
        for name, counts in tally.skills.items():
            skills.setdefault(name, Tally()).merge(counts)
        for name, counts in tally.servers.items():
            servers.setdefault(name, Tally()).merge(counts)
    if use_cache:
        save_cache(store, files)
    return {
        "schemaVersion": SCHEMA,
        "transcripts": len(paths),
        "unreadable": len(errors),
        "errors": errors[:8],
        "skills": {name: counts.document() for name, counts in skills.items()},
        "servers": {name: counts.document() for name, counts in servers.items()},
    }
