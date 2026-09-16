from __future__ import annotations

import contextlib
import fcntl
import hashlib
import json
import os
import sqlite3
import stat
import time
from collections.abc import Iterator
from pathlib import Path

from . import records

VERSION = 2
MAX_FILES = 8192
MAX_RECORD_BYTES = 4 * 1024 * 1024
CHUNK_BYTES = 8 * 1024 * 1024
BUDGET_SECONDS = 3.0
LOCK_SECONDS = 4.0
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


def xdg(variable: str, fallback: str) -> Path:
    value = os.environ.get(variable, "")
    return Path(value) if os.path.isabs(value) else Path.home() / fallback


def roots() -> dict[str, list[Path]]:
    claude = Path(os.environ.get("CLAUDE_CONFIG_DIR") or Path.home() / ".claude")
    codex = Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex")
    return {"claude": [claude / "projects"], "codex": [codex / "sessions", codex / "archived_sessions"]}


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
                if version not in (0, 1, VERSION):
                    raise sqlite3.DatabaseError("unsupported usage schema")
                if version == 0:
                    for statement in SCHEMA:
                        connection.execute(statement)
                if version < VERSION:
                    connection.execute(RETENTION)
                    connection.execute("CREATE TABLE failure (call TEXT PRIMARY KEY, at INTEGER NOT NULL) WITHOUT ROWID")
                    connection.execute("CREATE TABLE forgotten (identity BLOB PRIMARY KEY) WITHOUT ROWID")
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


def transcripts() -> tuple[list[tuple[str, str, os.stat_result]], int]:
    found: list[tuple[str, str, os.stat_result]] = []
    unreadable = 0
    for agent, directories in roots().items():
        count = 0
        for path in (path for directory in directories for path in directory.rglob("*.jsonl")):
            if count >= MAX_FILES:
                break
            try:
                status = path.stat()
            except OSError:
                unreadable += 1
                continue
            if stat.S_ISREG(status.st_mode):
                found.append((agent, str(path), status))
                count += 1
    found.sort(key=lambda entry: entry[2].st_mtime_ns, reverse=True)
    return found, unreadable


def read(agent: str, path: str, status: os.stat_result, row: tuple | None, deadline: float) -> records.Batch:
    restart = row is None or row[1:3] != (status.st_dev, status.st_ino) or status.st_size < row[5]
    batch = records.Batch(offset=0 if restart else row[5], project=None if restart else row[6])
    wanted, consume = (records.claude_line, records.claude) if agent == "claude" else (records.codex_line, records.codex)
    with open(path, "rb") as handle:
        handle.seek(max(0, batch.offset - 1))
        skipping = batch.offset > 0 and handle.read(1) != b"\n"
        start = batch.offset
        while True:
            if time.monotonic() >= deadline or batch.offset - start >= CHUNK_BYTES or len(batch.events) >= 1024:
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
            opening = restart and batch.first_at is None and b'"timestamp"' in raw
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


def clean(value: object) -> object:
    return value.encode("utf-8", "replace").decode("utf-8") if isinstance(value, str) else value


def identity(agent: str, call: str) -> bytes:
    return hashlib.sha256((agent + "\0" + call).encode("utf-8", "replace")).digest()


def project_id(connection: sqlite3.Connection, path: str | None) -> int | None:
    if path is None:
        return None
    connection.execute("INSERT OR IGNORE INTO project (path) VALUES (?)", (clean(path),))
    return connection.execute("SELECT id FROM project WHERE path = ?", (clean(path),)).fetchone()[0]


def write(connection: sqlite3.Connection, agent: str, path: str, status: os.stat_result, batch: records.Batch) -> None:
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
        connection.executemany("INSERT OR IGNORE INTO failure VALUES (?, ?)",
                               [(clean(call), at) for call, at in batch.failures
                                if at >= cutoff and (not forgotten or identity("claude", call) not in forgotten)])
        connection.execute("UPDATE event SET failed = 1 WHERE agent = 'claude' AND call IN (SELECT call FROM failure)")
        connection.execute("DELETE FROM failure WHERE EXISTS (SELECT 1 FROM event WHERE agent = 'claude' AND event.call = failure.call)")
        if covered:
            connection.execute("INSERT INTO coverage VALUES (?, ?) ON CONFLICT (agent) DO UPDATE "
                               "SET first_at = min(first_at, excluded.first_at)", (agent, max(cutoff, batch.first_at)))
        project = project_id(connection, batch.project) if covered else connection.execute(
            "SELECT (SELECT project FROM source WHERE path = ?)", (clean(path),)).fetchone()[0]
        connection.execute(
            "INSERT INTO source (agent, path, device, inode, size, mtime, offset, project) VALUES (?, ?, ?, ?, ?, ?, ?, ?) "
            "ON CONFLICT (path) DO UPDATE SET agent = excluded.agent, device = excluded.device, inode = excluded.inode, "
            "size = excluded.size, mtime = excluded.mtime, offset = excluded.offset, project = excluded.project",
            (agent, clean(path), status.st_dev, status.st_ino, status.st_size, status.st_mtime_ns, batch.offset,
             project),
        )


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
            if row and row[1:5] == (status.st_dev, status.st_ino, status.st_size, status.st_mtime_ns) and row[5] == status.st_size:
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
