from __future__ import annotations

import ctypes
import datetime as dt
import os
import re
import stat
import struct
from typing import Any

from fileblade_inventory import watch_path

MAX_ITEMS = 256
MAX_NAME = 256
MAX_DETAIL = 512
MAX_PATH = 4096
MAX_ENV_PATH = 4096
MAX_AGENTS = 16
MAX_DESCRIPTOR_BYTES = 64 * 1024
MAX_FRONTMATTER_BYTES = 32 * 1024
MAX_ENTRIES_PER_ROOT = 512
MAX_TOTAL_ENTRIES = 8192
MAX_ROOTS = 256
MAX_PROJECT_WALK = 32
MAX_PLUGINS = 256
MAX_CONFIG_BYTES = 256 * 1024
MAX_EXTENSIONS = 128
MAX_MANIFEST_BYTES = 64 * 1024

CONTROL = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")

def creation_time(path: str) -> str:
    try:
        statx = ctypes.CDLL(None, use_errno=True).statx
        statx.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_uint, ctypes.c_void_p)
        statx.restype = ctypes.c_int
        result = ctypes.create_string_buffer(256)
        if statx(-100, os.fsencode(path), 0x100, 0x800, ctypes.byref(result)) != 0:
            return ""
        mask = struct.unpack_from("I", result.raw, 0)[0]
        seconds = struct.unpack_from("q", result.raw, 80)[0]
        return dt.datetime.fromtimestamp(seconds).strftime("%Y-%m-%d %H:%M") if mask & 0x800 and seconds > 0 else ""
    except (AttributeError, OSError, struct.error, ValueError):
        return ""

def estimated_tokens(text: str) -> int:
    return (len(text.encode("utf-8", "replace")) + 3) // 4

def artifact_metrics(path: str, text: str, description: str = "") -> dict[str, Any]:
    try:
        metadata = os.stat(path)
    except OSError:
        return {}
    complete = metadata.st_size <= MAX_DESCRIPTOR_BYTES
    return {
        "updated": dt.datetime.fromtimestamp(metadata.st_mtime).strftime("%Y-%m-%d %H:%M"),
        "created": creation_time(path),
        "bytes": metadata.st_size,
        "characters": len(text) if complete else None,
        "words": len(re.findall(r"\w+", text, re.UNICODE)) if complete else None,
        "tokens": estimated_tokens(description) if complete else None,
        "fileTokens": estimated_tokens(text) if complete else None,
    }

def clean(value: Any, limit: int) -> str:
    text = "" if value is None else str(value)
    text = CONTROL.sub("", text).replace("\t", " ")
    text = re.sub(r"\s+", " ", text).strip()
    return text[:limit]

def env_path_value(environ: dict, name: str) -> str:
    value = environ.get(name, "")
    if not isinstance(value, str) or value == "":
        return ""
    if "\x00" in value or len(value) > MAX_ENV_PATH:
        return ""
    return value

def bounded_names(path: str, limit: int) -> tuple[list[str], bool]:
    watch_path(path, directory=True)
    names = []
    try:
        with os.scandir(path) as entries:
            for entry in entries:
                if len(names) >= limit:
                    return sorted(names), True
                names.append(entry.name)
    except OSError:
        pass
    return sorted(names), False

def read_descriptor(path: str, limit: int = MAX_DESCRIPTOR_BYTES, secure: bool = False) -> bytes | None:
    watch_path(path)
    flags = os.O_RDONLY | os.O_CLOEXEC | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        return None
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
            return None
        if secure and (metadata.st_uid != 0 or metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH)):
            return None
        return os.read(descriptor, limit)
    except OSError:
        return None
    finally:
        os.close(descriptor)

def read_document(path: str, limit: int = MAX_DESCRIPTOR_BYTES) -> str:
    data = read_descriptor(path, limit)
    return "" if data is None else data.decode("utf-8", errors="replace")

def read_secure_document(path: str, enforce: bool, limit: int = MAX_CONFIG_BYTES) -> str:
    data = read_descriptor(path, limit, secure=enforce)
    return "" if data is None else data.decode("utf-8", errors="replace")
