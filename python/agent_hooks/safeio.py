from __future__ import annotations

import ctypes
import datetime as dt
import hashlib
import json
import os
import re
import stat
import struct
import tomllib
from pathlib import Path
from typing import Any

from fileblade_paths import parse_path, wire
from fileblade_inventory import watch_path

MAX_FILE_BYTES = 256 * 1024
MAX_DIR_ENTRIES = 256
MAX_TRAVERSAL_DEPTH = 5
MAX_JSON_DEPTH = 12
MAX_ITEMS_PER_SOURCE = 200
MAX_SOURCES = 128
MAX_ROWS = 600
MAX_STDOUT_BYTES = 1024 * 1024
COMMENT_PATTERN = re.compile(r'("(?:\\.|[^"\\])*")|(//[^\n]*|/\*.*?\*/)', re.DOTALL)

def creation_time(path: Path) -> str:
    try:
        statx = ctypes.CDLL(None, use_errno=True).statx
        statx.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int,
                          ctypes.c_uint, ctypes.c_void_p]
        statx.restype = ctypes.c_int
        result = ctypes.create_string_buffer(256)
        if statx(-100, os.fsencode(path), 0x100, 0x800, ctypes.byref(result)) != 0:
            return ""
        mask = struct.unpack_from("I", result.raw, 0)[0]
        seconds = struct.unpack_from("q", result.raw, 80)[0]
        return dt.datetime.fromtimestamp(seconds).strftime("%Y-%m-%d %H:%M") if mask & 0x800 and seconds > 0 else ""
    except (AttributeError, OSError, OverflowError, struct.error, ValueError):
        return ""

def resolved_file(path: Path) -> Path | None:
    watch_path(path)
    try:
        resolved = path.resolve(strict=True)
        return resolved if stat.S_ISREG(resolved.lstat().st_mode) else None
    except (OSError, RuntimeError):
        return None

def artifact_metrics(path: Path, text: str) -> dict[str, Any]:
    target = resolved_file(path)
    if target is None:
        return {}
    metadata = target.lstat()
    encoded = text.encode("utf-8", "replace")
    countable = len(encoded) <= MAX_FILE_BYTES
    return {
        "updated": dt.datetime.fromtimestamp(metadata.st_mtime).strftime("%Y-%m-%d %H:%M"),
        "created": creation_time(target),
        "bytes": len(encoded),
        "characters": len(text) if countable else None,
        "words": len(re.findall(r"\w+", text, re.UNICODE)) if countable else None,
        "tokens": (len(encoded) + 3) // 4 if countable else None,
    }

class Budget:
    def __init__(self, sources: int = MAX_SOURCES, rows: int = MAX_ROWS) -> None:
        self.sources = sources
        self.rows = rows
        self.truncated = False
        self.hook_sources: dict[str, set[str]] = {}

    def note_source(self, agent: str, path) -> None:
        self.hook_sources.setdefault(str(agent), set()).add(str(path))

    def take_source(self) -> bool:
        if self.sources <= 0:
            self.truncated = True
            return False
        self.sources -= 1
        return True

    def take_row(self) -> bool:
        if self.rows <= 0:
            self.truncated = True
            return False
        self.rows -= 1
        return True

def expanded(path: str | os.PathLike[str]) -> Path:
    return Path(os.path.expanduser(parse_path(str(path))))

def stable_id(*parts: object) -> str:
    joined = "\u0000".join(str(part) for part in parts)
    return hashlib.sha256(joined.encode("utf-8", "surrogatepass")).hexdigest()[:16]

def digest(value: str) -> str:
    return hashlib.sha256(str(value).encode("utf-8", "replace")).hexdigest()[:8]

def secure_metadata(metadata: os.stat_result, owner_uid: int) -> bool:
    if metadata.st_uid != owner_uid:
        return False
    return not metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH)

def read_bytes(path: Path, owner_uid: int | None = None) -> bytes | None:
    target = resolved_file(path)
    if target is None:
        return None
    flags = os.O_RDONLY | os.O_CLOEXEC | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    try:
        descriptor = os.open(target, flags)
    except OSError:
        return None
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > MAX_FILE_BYTES:
            return None
        if owner_uid is not None and not secure_metadata(metadata, owner_uid):
            return None
        return os.read(descriptor, MAX_FILE_BYTES)
    except OSError:
        return None
    finally:
        os.close(descriptor)

def strip_comments(text: str) -> str:
    def replace(match: re.Match[str]) -> str:
        return match.group(1) if match.group(1) else " "

    return COMMENT_PATTERN.sub(replace, text)

def bounded_depth(value: Any, limit: int = MAX_JSON_DEPTH, level: int = 0) -> bool:
    if level > limit:
        return False
    if isinstance(value, dict):
        return all(bounded_depth(item, limit, level + 1) for item in value.values())
    if isinstance(value, list):
        return all(bounded_depth(item, limit, level + 1) for item in value)
    return True

def load_json(path: Path, allow_comments: bool = False, owner_uid: int | None = None) -> dict[str, Any] | None:
    raw = read_bytes(path, owner_uid)
    if raw is None:
        return None
    text = raw.decode("utf-8", "replace")
    if allow_comments:
        text = strip_comments(text)
    try:
        parsed = json.loads(text)
    except (ValueError, RecursionError):
        return None
    if not isinstance(parsed, dict) or not bounded_depth(parsed):
        return None
    return parsed

def load_toml(path: Path) -> dict[str, Any] | None:
    raw = read_bytes(path)
    if raw is None:
        return None
    try:
        parsed = tomllib.loads(raw.decode("utf-8", "replace"))
    except (ValueError, RecursionError):
        return None
    return parsed if bounded_depth(parsed) else None

def is_regular(path: Path) -> bool:
    return resolved_file(path) is not None

def is_plain_dir(path: Path) -> bool:
    watch_path(path, directory=True)
    try:
        metadata = path.lstat()
    except OSError:
        return False
    return stat.S_ISDIR(metadata.st_mode)

def scan_dir(directory: Path, suffix: str = ".json", depth: int = 1) -> list[Path]:
    depth = min(depth, MAX_TRAVERSAL_DEPTH)
    if depth <= 0 or not is_plain_dir(directory):
        return []
    found: list[Path] = []
    try:
        with os.scandir(directory) as entries:
            for count, entry in enumerate(entries):
                if count >= MAX_DIR_ENTRIES:
                    break
                child = Path(entry.path)
                if entry.is_symlink():
                    continue
                if entry.is_dir(follow_symlinks=False) and depth > 1:
                    found.extend(scan_dir(child, suffix, depth - 1))
                elif entry.is_file(follow_symlinks=False) and child.suffix == suffix:
                    found.append(child)
    except OSError:
        return found
    return sorted(found)[:MAX_DIR_ENTRIES]

def existing_file(path: Path) -> Path | None:
    return path if is_regular(path) else None

def serialized(payload: dict[str, Any]) -> str:
    return json.dumps(wire(payload), ensure_ascii=True, separators=(",", ":"))

def fit_payload(payload: dict[str, Any], limit: int = MAX_STDOUT_BYTES) -> str:
    text = serialized(payload)
    if len(text.encode("utf-8")) <= limit:
        return text
    items = list(payload.get("items") or [])
    low, high = 0, len(items)
    while low < high:
        middle = (low + high + 1) // 2
        payload["items"] = items[:middle]
        payload["count"] = middle
        payload["truncated"] = True
        if len(serialized(payload).encode("utf-8")) <= limit:
            low = middle
        else:
            high = middle - 1
    payload["items"] = items[:low]
    payload["count"] = low
    payload["truncated"] = True
    text = serialized(payload)
    if len(text.encode("utf-8")) <= limit:
        return text
    payload["items"] = []
    payload["count"] = 0
    return serialized(payload)
