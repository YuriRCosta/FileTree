from __future__ import annotations

import contextlib
import datetime as dt
import fcntl
import hashlib
import json
import os
import sqlite3
import stat
import time
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path

from . import records

VERSION = 3
MAX_FILES = 8192
MAX_RECORD_BYTES = 4 * 1024 * 1024
CHUNK_BYTES = 8 * 1024 * 1024
CHUNK_ROWS = 1024
BUDGET_SECONDS = 3.0
LOCK_SECONDS = 4.0
MAX_WATCH_DIRECTORIES = 96
SKILL_AGENTS = ("claude", "codex", "opencode", "copilot", "antigravity", "pi")
MCP_AGENTS = ("claude", "codex", "opencode", "copilot")
SCHEMA = (
    """CREATE TABLE source (
  id INTEGER PRIMARY KEY,
  agent TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  device INTEGER NOT NULL,
  inode INTEGER NOT NULL,
  size INTEGER NOT NULL,
  mtime INTEGER NOT NULL,
  offset INTEGER NOT NULL,
  project INTEGER REFERENCES project(id)
)""",
    "CREATE TABLE project (id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE)",
    "CREATE TABLE coverage (agent TEXT PRIMARY KEY, first_at INTEGER NOT NULL)",
    """CREATE TABLE event (
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
) WITHOUT ROWID""",
    "CREATE INDEX event_time ON event (kind, at)",
    "CREATE INDEX event_name ON event (kind, server, name, at)",
)
RETENTION = "CREATE TABLE retention (id INTEGER PRIMARY KEY CHECK (id = 1), before INTEGER NOT NULL)"
FAILURE = "CREATE TABLE failure (call TEXT PRIMARY KEY, at INTEGER NOT NULL) WITHOUT ROWID"
FAILURE_AGENT = "ALTER TABLE failure ADD COLUMN agent TEXT NOT NULL DEFAULT 'claude'"


@dataclass(frozen=True)
class Identity:
    st_dev: int
    st_ino: int
    st_size: int
    st_mtime_ns: int


@dataclass(frozen=True)
class Source:
    agent: str
    directory: Path
    pattern: str
    recursive: bool


def xdg(variable: str, fallback: str) -> Path:
    value = os.environ.get(variable, "")
    return Path(value) if os.path.isabs(value) else Path.home() / fallback


def home(variable: str, fallback: str) -> Path:
    value = os.environ.get(variable, "")
    return Path(value) if os.path.isabs(value) else Path.home() / fallback


def sources() -> list[Source]:
    claude = home("CLAUDE_CONFIG_DIR", ".claude")
    codex = home("CODEX_HOME", ".codex")
    copilot = home("COPILOT_HOME", ".copilot")
    antigravity = home("GEMINI_HOME", ".gemini") / "antigravity-cli"
    pi = home("PI_HOME", ".pi") / "agent"
    return [
        Source("claude", claude / "projects", "*.jsonl", True),
        Source("codex", codex / "sessions", "*.jsonl", True),
        Source("codex", codex / "archived_sessions", "*.jsonl", True),
        Source("opencode", xdg("XDG_DATA_HOME", ".local/share") / "opencode", "opencode*.db", False),
        Source("copilot", copilot / "session-state", "*/events.jsonl", False),
        Source("antigravity", antigravity, "history.jsonl", False),
        Source("antigravity", antigravity / "brain", "*/.system_generated/logs/transcript_full.jsonl", False),
        Source("pi", pi / "sessions", "*.jsonl", True),
    ]


def roots() -> dict[str, list[Path]]:
    found: dict[str, list[Path]] = {}
    for source in sources():
        found.setdefault(source.agent, []).append(source.directory)
    return found


def recent_directories(directory: Path, limit: int, suffix: str = "") -> list[Path]:
    try:
        children = [child for child in directory.iterdir() if child.is_dir()]
    except OSError:
        return []
    ranked = []
    for child in children:
        try:
            ranked.append((child.stat().st_mtime_ns, child))
        except OSError:
            continue
    ranked.sort(key=lambda entry: entry[0], reverse=True)
    picked = []
    for _, child in ranked[:limit]:
        target = child / suffix if suffix else child
        if target.is_dir():
            picked.append(target)
    return picked


def watch_paths() -> list[str]:
    found: list[Path] = []
    today = dt.date.today()
    for source in sources():
        if not source.directory.is_dir():
            continue
        found.append(source.directory)
        if source.agent == "claude":
            found.extend(recent_directories(source.directory, 24))
        elif source.agent == "codex":
            for day in (today, today - dt.timedelta(days=1)):
                daily = source.directory / f"{day.year:04d}" / f"{day.month:02d}" / f"{day.day:02d}"
                if daily.is_dir():
                    found.append(daily)
        elif source.agent == "copilot":
            found.extend(recent_directories(source.directory, 16))
        elif source.agent == "antigravity" and source.directory.name == "brain":
            found.extend(recent_directories(source.directory, 8, ".system_generated/logs"))
        elif source.agent == "pi":
            found.extend(recent_directories(source.directory, 8))
    unique: list[str] = []
    for path in found:
        try:
            resolved = str(path.resolve())
        except (OSError, RuntimeError):
            continue
        if resolved not in unique:
            unique.append(resolved)
    return unique[:MAX_WATCH_DIRECTORIES]


def connect(directory: Path) -> sqlite3.Connection:
    for name in ("agent-usage.json", "agent-usage.tmp"):
        with contextlib.suppress(OSError):
            (xdg("XDG_CACHE_HOME", ".cache") / "omarchy" / "fileblade" / name).unlink()
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    path = directory / "agent-usage.sqlite3"
    os.close(os.open(path, os.O_RDWR | os.O_CREAT | os.O_CLOEXEC, 0o600))
    connection = sqlite3.connect(path, timeout=5, isolation_level=None)
    try:
        connection.execute("PRAGMA busy_timeout = 5000")
        connection.execute("PRAGMA journal_mode = WAL")
        if connection.execute("PRAGMA user_version").fetchone()[0] != VERSION:
            with connection:
                connection.execute("BEGIN IMMEDIATE")
                version = connection.execute("PRAGMA user_version").fetchone()[0]
                if version not in (0, 1, 2, VERSION):
                    raise sqlite3.DatabaseError("unsupported usage schema")
                if version == 0:
                    for statement in SCHEMA:
                        connection.execute(statement)
                if version < 2:
                    connection.execute(RETENTION)
                    connection.execute(FAILURE)
                    connection.execute("CREATE TABLE forgotten (identity BLOB PRIMARY KEY) WITHOUT ROWID")
                if version < 3:
                    connection.execute(FAILURE_AGENT)
                connection.execute(f"PRAGMA user_version = {VERSION}")
    except sqlite3.Error:
        connection.close()
        raise
    return connection


def acquire(descriptor: int) -> bool:
    deadline = time.monotonic() + LOCK_SECONDS
    while True:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return True
        except BlockingIOError:
            if time.monotonic() >= deadline:
                return False
            time.sleep(0.05)


def sidecar_identity(path: Path, status: os.stat_result) -> Identity:
    size, mtime = status.st_size, status.st_mtime_ns
    for suffix in ("-wal", "-shm"):
        try:
            extra = Path(str(path) + suffix).stat()
        except OSError:
            continue
        size += extra.st_size
        mtime = max(mtime, extra.st_mtime_ns)
    return Identity(status.st_dev, status.st_ino, size, mtime)


def transcripts() -> tuple[list[tuple[str, str, Identity]], int]:
    found: list[tuple[str, str, Identity]] = []
    unreadable = 0
    counts: dict[str, int] = {}
    for source in sources():
        walker = source.directory.rglob if source.recursive else source.directory.glob
        try:
            candidates = list(walker(source.pattern))
        except OSError:
            unreadable += 1
            continue
        for path in candidates:
            if counts.get(source.agent, 0) >= MAX_FILES:
                break
            try:
                status = path.stat()
            except OSError:
                unreadable += 1
                continue
            if not stat.S_ISREG(status.st_mode):
                continue
            identity = sidecar_identity(path, status) if source.agent == "opencode" else Identity(
                status.st_dev, status.st_ino, status.st_size, status.st_mtime_ns)
            found.append((source.agent, str(path), identity))
            counts[source.agent] = counts.get(source.agent, 0) + 1
    found.sort(key=lambda entry: entry[2].st_mtime_ns, reverse=True)
    return found, unreadable


def read(agent: str, path: str, status: Identity, row: tuple | None, deadline: float) -> records.Batch:
    if agent == "opencode":
        return read_sqlite(path, row, deadline)
    restart = row is None or row[1:3] != (status.st_dev, status.st_ino) or status.st_size < row[5]
    batch = records.Batch(offset=0 if restart else row[5], project=None if restart else row[6], source=path)
    wanted, consume = records.LINE_READERS[agent]
    with open(path, "rb") as handle:
        handle.seek(max(0, batch.offset - 1))
        skipping = batch.offset > 0 and handle.read(1) != b"\n"
        start = batch.offset
        while True:
            if time.monotonic() >= deadline or batch.offset - start >= CHUNK_BYTES or len(batch.events) >= CHUNK_ROWS:
                batch.pending = batch.offset < status.st_size
                break
            raw = handle.readline(MAX_RECORD_BYTES + 1)
            if not raw:
                break
            if skipping or len(raw) > MAX_RECORD_BYTES:
                skipping = not raw.endswith(b"\n")
                batch.offset += len(raw)
                continue
            if not raw.endswith(b"\n"):
                break
            opening = restart and batch.first_at is None and (b'"timestamp"' in raw or b'"created_at"' in raw)
            batch.offset += len(raw)
            if not (opening or wanted(raw)):
                continue
            try:
                record = json.loads(raw)
            except (ValueError, RecursionError):
                continue
            if isinstance(record, dict):
                consume(record, batch)
    return batch


def opencode_tables(connection: sqlite3.Connection) -> set[str]:
    return {name for (name,) in connection.execute("SELECT name FROM sqlite_master WHERE type = 'table'")}


def read_sqlite(path: str, row: tuple | None, deadline: float) -> records.Batch:
    watermark = 0 if row is None else max(0, row[5])
    batch = records.Batch(offset=watermark, project=None, source=path)
    try:
        connection = sqlite3.connect(path, timeout=1, isolation_level=None)
    except sqlite3.Error as error:
        raise OSError(str(error)) from error
    try:
        connection.execute("PRAGMA query_only = ON")
        connection.execute("PRAGMA busy_timeout = 1000")
        tables = opencode_tables(connection)
        if "part" in tables and "session" in tables:
            query = ("SELECT part.id, part.time_updated, part.time_created, part.data, session.directory FROM part "
                     "LEFT JOIN session ON session.id = part.session_id WHERE part.time_updated > ? "
                     "AND instr(part.data, '\"tool\"') > 0 ORDER BY part.time_updated LIMIT ?")
            table = "session"
        elif "session_message" in tables and "session_v2" in tables:
            query = ("SELECT m.id, m.time_updated, m.time_created, m.data, s.directory FROM session_message m "
                     "LEFT JOIN session_v2 s ON s.id = m.session_id WHERE m.time_updated > ? AND m.type = 'assistant' "
                     "AND instr(m.data, '\"tool\"') > 0 ORDER BY m.time_updated LIMIT ?")
            table = "session_v2"
        else:
            return batch
        if watermark == 0:
            earliest = connection.execute(f"SELECT min(time_created) FROM {table}").fetchone()[0]
            if isinstance(earliest, int):
                batch.note(earliest)
        rows = connection.execute(query, (watermark, CHUNK_ROWS + 1)).fetchall()
    except sqlite3.Error as error:
        raise OSError(str(error)) from error
    finally:
        connection.close()
    batch.pending = len(rows) > CHUNK_ROWS
    for identity, updated, created, data, directory in rows[:CHUNK_ROWS]:
        if time.monotonic() >= deadline:
            batch.pending = True
            break
        batch.offset = max(batch.offset, int(updated or 0))
        try:
            document = json.loads(data) if isinstance(data, (str, bytes)) else None
        except (ValueError, RecursionError):
            continue
        if not isinstance(document, dict):
            continue
        project = directory if isinstance(directory, str) and directory else None
        if table == "session":
            moment = records.mapping(records.mapping(document.get("state")).get("time"))
            at = moment.get("end") or moment.get("start") or created
            if isinstance(at, int) and not isinstance(at, bool):
                records.opencode_part(document, str(identity), batch.note(at), project, batch)
            continue
        for index, item in enumerate(document.get("content") if isinstance(document.get("content"), list) else []):
            if not isinstance(item, dict) or item.get("type") != "tool":
                continue
            moment = records.mapping(item.get("time"))
            at = moment.get("completed") or moment.get("created") or created
            if not isinstance(at, int) or isinstance(at, bool):
                continue
            shaped = {"type": "tool", "callID": records.text(item.get("id")) or f"{identity}:{index}",
                      "tool": records.text(item.get("name")), "state": records.mapping(item.get("state"))}
            records.opencode_part(shaped, f"{identity}:{index}", batch.note(at), project, batch)
    return batch


def clean(value: object) -> object:
    return value.encode("utf-8", "replace").decode("utf-8") if isinstance(value, str) else value


def identity(agent: str, call: str) -> bytes:
    return hashlib.sha256((agent + "\0" + call).encode("utf-8", "replace")).digest()


def project_id(connection: sqlite3.Connection, path: str | None) -> int | None:
    if path is None:
        return None
    connection.execute("INSERT OR IGNORE INTO project (path) VALUES (?)", (clean(path),))
    return connection.execute("SELECT id FROM project WHERE path = ?", (clean(path),)).fetchone()[0]


def write(connection: sqlite3.Connection, agent: str, path: str, status: Identity, batch: records.Batch) -> None:
    with connection:
        connection.execute("BEGIN IMMEDIATE")
        cutoff = connection.execute("SELECT coalesce(max(before), -9223372036854775808) FROM retention").fetchone()[0]
        forgotten = {row[0] for row in connection.execute("SELECT identity FROM forgotten")}
        events = [event for event in batch.events if event[2] >= cutoff and (not forgotten or identity(event[0], event[1]) not in forgotten)]
        covered = batch.first_at is not None and (bool(events) or (not batch.events and batch.first_at >= cutoff))
        connection.executemany(
            "INSERT OR IGNORE INTO event VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [(*map(clean, event[:8]), project_id(connection, event[8]), event[9]) for event in events],
        )
        if events or batch.failures:
            connection.executemany("INSERT OR IGNORE INTO failure (call, at, agent) VALUES (?, ?, ?)",
                                   [(clean(call), at, agent) for call, at in batch.failures
                                    if at >= cutoff and (not forgotten or identity(agent, call) not in forgotten)])
            connection.execute("UPDATE event SET failed = 1 WHERE (agent, call) IN (SELECT agent, call FROM failure)")
            connection.execute("DELETE FROM failure WHERE EXISTS (SELECT 1 FROM event WHERE event.agent = failure.agent AND event.call = failure.call)")
        if covered:
            connection.execute("INSERT INTO coverage VALUES (?, ?) ON CONFLICT (agent) DO UPDATE "
                               "SET first_at = min(first_at, excluded.first_at)", (agent, max(cutoff, batch.first_at)))
        project = project_id(connection, batch.project) if covered else connection.execute(
            "SELECT (SELECT project FROM source WHERE path = ?)", (clean(path),)).fetchone()[0]
        size = -1 if agent == "opencode" and batch.pending else status.st_size
        connection.execute(
            "INSERT INTO source (agent, path, device, inode, size, mtime, offset, project) VALUES (?, ?, ?, ?, ?, ?, ?, ?) "
            "ON CONFLICT (path) DO UPDATE SET agent = excluded.agent, device = excluded.device, inode = excluded.inode, "
            "size = excluded.size, mtime = excluded.mtime, offset = excluded.offset, project = excluded.project",
            (agent, clean(path), status.st_dev, status.st_ino, size, status.st_mtime_ns, batch.offset, project),
        )


def complete(agent: str, row: tuple, status: Identity) -> bool:
    if row[1:5] != (status.st_dev, status.st_ino, status.st_size, status.st_mtime_ns):
        return False
    return agent == "opencode" or row[5] == status.st_size


def ingest(connection: sqlite3.Connection, directory: Path) -> tuple[bool, int]:
    deadline = time.monotonic() + BUDGET_SECONDS
    descriptor = os.open(directory / "agent-usage.sqlite3.lock", os.O_RDWR | os.O_CREAT | os.O_CLOEXEC, 0o600)
    try:
        if not acquire(descriptor):
            return True, 0
        found, unreadable = transcripts()
        known = {row[0]: row for row in connection.execute(
            "SELECT source.path, device, inode, size, mtime, offset, project.path FROM source "
            "LEFT JOIN project ON project.id = source.project")}
        pending = False
        for agent, path, status in found:
            row = known.get(clean(path))
            if row and complete(agent, row, status):
                continue
            if time.monotonic() >= deadline:
                pending = True
                continue
            try:
                while True:
                    batch = read(agent, path, status, row, deadline)
                    write(connection, agent, path, status, batch)
                    if not batch.pending or time.monotonic() >= deadline:
                        pending = pending or batch.pending
                        break
                    row = (path, status.st_dev, status.st_ino, status.st_size, status.st_mtime_ns, batch.offset, batch.project)
            except OSError:
                unreadable += 1
                continue
        seen = {clean(path) for _, path, _ in found}
        vanished = [(path,) for path in known if path not in seen and not os.path.exists(path)]
        if vanished and not pending:
            with connection:
                connection.execute("BEGIN IMMEDIATE")
                connection.executemany("DELETE FROM source WHERE path = ?", vanished)
        return pending, unreadable
    finally:
        os.close(descriptor)


@contextlib.contextmanager
def session(ingest_history: bool = True) -> Iterator[tuple[sqlite3.Connection, tuple[bool, int]]]:
    directory = xdg("XDG_STATE_HOME", ".local/state") / "omarchy" / "fileblade"
    connection = connect(directory)
    try:
        yield connection, ingest(connection, directory) if ingest_history else (False, 0)
    finally:
        connection.close()
