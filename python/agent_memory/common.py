from __future__ import annotations

import ctypes
import datetime as dt
import errno
import hashlib
import json
import os
import re
import stat
import struct
import tomllib
import unicodedata
from pathlib import Path
from typing import Any

from fileblade_paths import parse_path
from fileblade_inventory import watch_path

SCHEMA_VERSION = 1
MAX_DESCRIPTOR_BYTES = 8 * 1024
MAX_FILE_BYTES = 4 * 1024 * 1024
MAX_ITEMS = 1000
MAX_SOURCES = 512
MAX_DIR_ENTRIES = 512
MAX_RULE_DEPTH = 4
MAX_ANCESTORS = 24
MAX_EXTRA_ROOTS = 16
MAX_DETAIL_CHARS = 160
MAX_NAME_CHARS = 120
MAX_CONFIG_BYTES = 64 * 1024
MAX_GLOB_MATCHES = 64
MAX_FALLBACK_NAMES = 8
MAX_INSTRUCTION_ENTRIES = 32
MAX_ENV_DIRS = 8
MAX_CONTEXT_NAMES = 8
MAX_EXTENSIONS = 128
MAX_MANIFEST_BYTES = 64 * 1024
MAX_BASENAME_CHARS = 255
MAX_DISCOVERY_DIRS = 200
MAX_ENV_PATH_CHARS = 4096
DEFAULT_DISCOVERY_MAX_DIRS = 200
DEFAULT_BOUNDARY_MARKERS = (".git",)
IGNORED_DISCOVERY_DIRS = frozenset({
    ".git", ".hg", ".svn", "node_modules", "dist", "build", "target", "vendor",
    "__pycache__", ".venv", "venv", ".cache", ".next", ".tox", ".mypy_cache",
})
UNSAFE_CATEGORIES = ("Cc", "Cf", "Cs")
REMOTE_PREFIXES = ("http://", "https://", "ftp://", "//")


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
    except (AttributeError, OSError, struct.error, TypeError, ValueError):
        return ""


def artifact_metrics(path: Path, text: str, size: int) -> dict[str, Any]:
    try:
        metadata = path.stat()
    except OSError:
        return {}
    complete = size <= MAX_DESCRIPTOR_BYTES
    encoded = text.encode("utf-8", "replace") if complete else b""
    return {
        "updated": dt.datetime.fromtimestamp(metadata.st_mtime).strftime("%Y-%m-%d %H:%M"),
        "created": creation_time(path),
        "bytes": size,
        "characters": len(text) if complete else None,
        "words": len(re.findall(r"\w+", text, re.UNICODE)) if complete else None,
        "tokens": (len(encoded) + 3) // 4 if complete else None,
    }


PROJECT_MARKERS = (".git", ".claude", ".agents", ".codex", ".opencode", ".pi", ".github")

class Budget:
    def __init__(self, items: int = MAX_ITEMS, sources: int = MAX_SOURCES) -> None:
        self.items = items
        self.sources = sources
        self.truncated = False

    def take_source(self) -> bool:
        if self.sources <= 0:
            self.truncated = True
            return False
        self.sources -= 1
        return True

    def take_item(self) -> bool:
        if self.items <= 0:
            self.truncated = True
            return False
        self.items -= 1
        return True

def expanded(path: str | os.PathLike[str]) -> Path:
    return Path(os.path.expanduser(parse_path(str(path))))

def realpath_of(path: Path) -> str:
    try:
        return str(path.resolve(strict=False))
    except OSError:
        return str(path)

def stable_id(realpath: str) -> str:
    return hashlib.sha256(os.fsencode(realpath)).hexdigest()[:16]

def open_regular(path: Path) -> tuple[int, int] | None:
    watch_path(path)
    flags = os.O_RDONLY | os.O_CLOEXEC | getattr(os, "O_NONBLOCK", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        return None
    try:
        metadata = os.fstat(descriptor)
    except OSError:
        os.close(descriptor)
        return None
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > MAX_FILE_BYTES:
        os.close(descriptor)
        return None
    return descriptor, metadata.st_size

def descriptor_head(path: Path) -> tuple[str, int] | None:
    opened = open_regular(path)
    if opened is None:
        return None
    descriptor, size = opened
    try:
        raw = os.read(descriptor, MAX_DESCRIPTOR_BYTES)
    except OSError:
        return None
    finally:
        os.close(descriptor)
    return raw.decode("utf-8", "replace"), size

def read_capped(path: Path, cap: int = MAX_CONFIG_BYTES) -> str:
    opened = open_regular(path)
    if opened is None:
        return ""
    descriptor, _ = opened
    try:
        return os.read(descriptor, cap).decode("utf-8", "replace")
    except OSError:
        return ""
    finally:
        os.close(descriptor)

def securely_owned(path: Path) -> bool:
    try:
        metadata = path.lstat()
    except OSError:
        return False
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 0:
        return False
    return not bool(metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH))

def open_no_follow(path: Path, cap: int) -> str:
    watch_path(path)
    flags = os.O_RDONLY | os.O_CLOEXEC | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        return ""
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > cap:
            return ""
        return os.read(descriptor, cap).decode("utf-8", "replace")
    except OSError:
        return ""
    finally:
        os.close(descriptor)

def load_json_secure(path: Path, enforce: bool, cap: int = MAX_CONFIG_BYTES) -> dict[str, Any]:
    if enforce and not securely_owned(path):
        return {}
    text = open_no_follow(path, cap)
    if not text:
        return {}
    try:
        parsed = json.loads(text)
    except ValueError:
        return {}
    return parsed if isinstance(parsed, dict) else {}

def load_json_document(path: Path) -> dict[str, Any]:
    raw = read_capped(path)
    if not raw:
        return {}
    try:
        parsed = json.loads(raw)
    except ValueError:
        return {}
    return parsed if isinstance(parsed, dict) else {}

def load_toml_document(path: Path) -> dict[str, Any]:
    raw = read_capped(path)
    if not raw:
        return {}
    try:
        parsed = tomllib.loads(raw)
    except (ValueError, tomllib.TOMLDecodeError):
        return {}
    return parsed if isinstance(parsed, dict) else {}

def safe_basename(value: object) -> str:
    if not isinstance(value, str):
        return ""
    name = value
    if not name.strip() or len(name) > MAX_BASENAME_CHARS:
        return ""
    if name in (".", ".."):
        return ""
    if "/" in name or "\\" in name or "\x00" in name:
        return ""
    for character in name:
        if unicodedata.category(character) in UNSAFE_CATEGORIES:
            return ""
    return name

def bounded_directories(root: Path, limit: int, follow_symlinks: bool = False) -> list[Path]:
    watch_path(root, directory=True)
    found: list[Path] = []
    try:
        with os.scandir(root) as entries:
            for count, entry in enumerate(entries):
                if count >= MAX_DIR_ENTRIES or len(found) >= limit:
                    break
                if entry.is_dir(follow_symlinks=follow_symlinks):
                    found.append(Path(entry.path))
    except OSError:
        return []
    return sorted(found)

def is_remote(value: str) -> bool:
    lowered = value.strip().lower()
    return lowered.startswith(REMOTE_PREFIXES)

def bounded_glob(base: Path, pattern: str) -> list[Path]:
    watch_path(base, directory=True)
    cleaned = str(pattern or "").strip()
    if not cleaned or cleaned.startswith("/") or cleaned.startswith("~") or ".." in cleaned:
        return []
    parts = Path(cleaned).parts
    if len(parts) > 32 or any("**" in part and part != "**" for part in parts):
        return []
    found: set[Path] = set()
    root = Path(realpath_of(base))
    pending = [(base, 0)]
    visited: set[tuple[Path, int]] = set()
    while pending and len(visited) < MAX_DISCOVERY_DIRS and len(found) < MAX_GLOB_MATCHES:
        directory, index = pending.pop(0)
        if index >= len(parts):
            continue
        resolved = Path(realpath_of(directory))
        key = (resolved, index)
        if (resolved != root and root not in resolved.parents) or key in visited:
            continue
        visited.add(key)
        watch_path(directory, directory=True)
        part = parts[index]
        if part == "**":
            pending.append((directory, index + 1))
        try:
            with os.scandir(directory) as entries:
                for count, entry in enumerate(entries):
                    if count >= MAX_DIR_ENTRIES or len(found) >= MAX_GLOB_MATCHES:
                        break
                    candidate = directory / entry.name
                    if part != "**" and not fnmatch_like(entry.name, part):
                        continue
                    if part != "**" and index == len(parts) - 1 and entry.is_file():
                        target = Path(realpath_of(candidate))
                        if root in target.parents:
                            found.add(candidate)
                    elif entry.is_dir() and len(pending) + len(visited) < MAX_DISCOVERY_DIRS:
                        pending.append((candidate, index if part == "**" else index + 1))
        except OSError:
            continue
    return sorted(found)

def matches_any(path: Path, patterns: list[str]) -> bool:
    target = str(path)
    resolved = realpath_of(path)
    for pattern in patterns:
        cleaned = str(pattern or "").strip()
        if not cleaned:
            continue
        for candidate in (target, resolved):
            try:
                if Path(candidate).match(cleaned) or fnmatch_like(candidate, cleaned):
                    return True
            except (ValueError, IndexError):
                continue
    return False

def fnmatch_like(value: str, pattern: str) -> bool:
    from fnmatch import fnmatchcase

    return fnmatchcase(value, pattern)

def summary_line(text: str) -> str:
    in_frontmatter = False
    for index, line in enumerate(text.splitlines()):
        stripped = line.strip()
        if index == 0 and stripped == "---":
            in_frontmatter = True
            continue
        if in_frontmatter:
            if stripped == "---":
                in_frontmatter = False
            continue
        if not stripped or stripped.startswith(("<!--", "```")):
            continue
        cleaned = stripped.lstrip("#").strip()
        if cleaned:
            return cleaned[:MAX_DETAIL_CHARS]
    return ""

def frontmatter_field(text: str, key: str) -> str:
    lines = text.splitlines()
    if not lines or lines[0].strip() != "---":
        return ""
    for line in lines[1:]:
        stripped = line.strip()
        if stripped == "---":
            return ""
        name, separator, value = stripped.partition(":")
        if separator and name.strip() == key:
            return value.strip().strip('"').strip("'")[:MAX_DETAIL_CHARS]
    return ""

def listed_files(directory: Path, suffixes: tuple[str, ...] = (".md",)) -> list[Path]:
    watch_path(directory, directory=True)
    try:
        with os.scandir(directory) as entries:
            found = []
            for count, entry in enumerate(entries):
                if count >= MAX_DIR_ENTRIES:
                    break
                if entry.is_file() and Path(entry.name).suffix in suffixes:
                    found.append(Path(entry.path))
    except OSError:
        return []
    return sorted(found)

def walked_files(directory: Path, suffixes: tuple[str, ...] = (".md",), depth: int = MAX_RULE_DEPTH) -> list[Path]:
    watch_path(directory, directory=True)
    found: list[Path] = []
    visited: set[tuple[int, int]] = set()
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC

    def walk(directory_fd: int, current: Path, level: int) -> None:
        if level > depth or len(found) >= MAX_DIR_ENTRIES:
            return
        watch_path(current, directory=True)
        try:
            opened = os.fstat(directory_fd)
            key = (opened.st_dev, opened.st_ino)
            if key in visited:
                return
            visited.add(key)
            with os.scandir(directory_fd) as entries:
                for count, entry in enumerate(entries):
                    if count >= MAX_DIR_ENTRIES or len(found) >= MAX_DIR_ENTRIES:
                        break
                    child = current / entry.name
                    if entry.is_dir(follow_symlinks=False):
                        try:
                            child_fd = os.open(entry.name, flags, dir_fd=directory_fd)
                        except OSError:
                            continue
                        try:
                            current_stat = os.stat(
                                entry.name, dir_fd=directory_fd, follow_symlinks=False)
                            child_stat = os.fstat(child_fd)
                            if (current_stat.st_dev, current_stat.st_ino) != (
                                    child_stat.st_dev, child_stat.st_ino):
                                continue
                            walk(child_fd, child, level + 1)
                        except OSError:
                            continue
                        finally:
                            os.close(child_fd)
                    elif entry.is_file() and child.suffix in suffixes:
                        found.append(child)
        except OSError:
            return

    try:
        root_fd = os.open(directory, flags)
    except OSError:
        return []
    try:
        walk(root_fd, directory, 0)
    finally:
        os.close(root_fd)
    return sorted(found)

def ancestors_of(start: Path, stop: Path | None = None) -> list[Path]:
    chain = [start, *start.parents][:MAX_ANCESTORS]
    if stop is None:
        return chain
    bounded = []
    for candidate in chain:
        bounded.append(candidate)
        if candidate == stop:
            break
    return bounded

def project_root(start: str, home: Path) -> str:
    if not start:
        return ""
    current = expanded(start)
    try:
        if current.is_file():
            current = current.parent
    except OSError:
        return ""
    for candidate in ancestors_of(current):
        if candidate == home.parent:
            break
        for marker in PROJECT_MARKERS:
            try:
                if (candidate / marker).exists():
                    return str(candidate)
            except OSError:
                continue
    return ""

def env_path_value(name: str, environ: dict[str, str]) -> str:
    value = environ.get(name, "")
    if not isinstance(value, str) or value == "":
        return ""
    if "\x00" in value or len(value) > MAX_ENV_PATH_CHARS:
        return ""
    return value

def env_path(name: str, environ: dict[str, str]) -> Path | None:
    value = env_path_value(name, environ)
    return expanded(value) if value else None

def readable(path: Path) -> bool:
    watch_path(path)
    try:
        return os.access(path, os.R_OK)
    except OSError:
        return False

def missing_is_fine(error: OSError) -> bool:
    return error.errno in (errno.ENOENT, errno.ENOTDIR, errno.EACCES, errno.ELOOP)

def bounded_document(payload: dict[str, Any], budget: Budget) -> dict[str, Any]:
    payload["schemaVersion"] = SCHEMA_VERSION
    payload["truncated"] = budget.truncated
    return payload
