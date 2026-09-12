from __future__ import annotations

from typing import Any

from .safeio import digest

SAFE_FIELD_NAMES = (
    "type", "command", "bash", "powershell", "url", "headers", "env", "allowedEnvVars",
    "cwd", "timeout", "timeoutSec", "prompt", "matcher", "name", "description",
    "statusMessage", "sequential", "enabled",
)
LABEL_FIELDS = ("statusMessage", "name", "description")
MAX_LABEL_CHARS = 80
MAX_BASENAME_CHARS = 64
NUMERIC_FIELDS = ("timeout", "timeoutSec")
REDACTED = "[redacted]"

def field_inventory(entry: dict[str, Any]) -> list[str]:
    return sorted(str(key) for key in entry.keys() if isinstance(key, str))[:24]

def safe_type(entry: dict[str, Any]) -> str:
    value = entry.get("type")
    if not isinstance(value, str):
        return "command" if any(key in entry for key in ("command", "bash", "powershell")) else "unknown"
    cleaned = value.strip().lower()
    return cleaned if cleaned in ("command", "http", "prompt", "script", "mcp_tool", "agent") else "unknown"

def safe_timeout(entry: dict[str, Any]) -> int | None:
    for field in NUMERIC_FIELDS:
        value = entry.get(field)
        if isinstance(value, bool):
            continue
        if isinstance(value, (int, float)) and 0 < float(value) < 86400:
            return int(value)
    return None

def payload_material(entry: dict[str, Any]) -> str:
    parts = []
    for field in ("command", "bash", "powershell", "url", "prompt"):
        value = entry.get(field)
        if isinstance(value, str):
            parts.append(field + ":" + value)
    return " ".join(parts)

def primary_payload(entry: dict[str, Any]) -> str:
    for field in ("command", "bash", "powershell", "url", "prompt"):
        value = entry.get(field)
        if isinstance(value, str) and value:
            return value
    return ""

def payload_digest(entry: dict[str, Any]) -> str:
    primary = primary_payload(entry)
    return digest(primary) if primary else ""

def env_count(entry: dict[str, Any]) -> int:
    total = 0
    value = entry.get("env")
    if isinstance(value, dict):
        total += len(value)
    allowed = entry.get("allowedEnvVars")
    if isinstance(allowed, list):
        total += len(allowed)
    return total

def matcher_shape(entry: dict[str, Any]) -> str:
    value = entry.get("matcher")
    if value is None:
        return "none"
    if not isinstance(value, str):
        return "invalid"
    if value.strip() == "":
        return "empty"
    return "pattern" if any(token in value for token in ".*+?[]()|\\^$") else "literal"

def enabled_state(entry: dict[str, Any], fallback: Any = None) -> bool | None:
    value = entry.get("enabled", fallback)
    return value if isinstance(value, bool) else None

def clean_label(value: Any) -> str:
    if not isinstance(value, str):
        return ""
    text = "".join(ch for ch in " ".join(value.split()) if ch.isprintable())
    return text[:MAX_LABEL_CHARS].strip()

def command_basename(entry: dict[str, Any]) -> str:
    for field in ("command", "bash", "powershell"):
        value = entry.get(field)
        if not isinstance(value, str) or not value.strip():
            continue
        token = value.strip().split()[0].strip("\"'")
        if "/" not in token:
            return ""
        base = token.rsplit("/", 1)[-1]
        if base in ("", ".", "..") or len(base) > MAX_BASENAME_CHARS:
            return ""
        if all(ch.isprintable() and not ch.isspace() for ch in base):
            return base
        return ""
    return ""

def safe_label(entry: dict[str, Any]) -> tuple[str, str]:
    for field in LABEL_FIELDS:
        text = clean_label(entry.get(field))
        if text:
            return text, field
    base = command_basename(entry)
    return (base, "command") if base else ("", "")

def safe_summary(entry: dict[str, Any]) -> dict[str, Any]:
    material = payload_material(entry)
    label, label_source = safe_label(entry)
    return {
        "label": label,
        "labelSource": label_source,
        "type": safe_type(entry),
        "fields": field_inventory(entry),
        "digest": payload_digest(entry),
        "payloadBytes": len(material.encode("utf-8", "replace")),
        "envCount": env_count(entry),
        "matcher": matcher_shape(entry),
        "timeoutSeconds": safe_timeout(entry),
        "hasCondition": "if" in entry,
    }

def assert_safe(row: dict[str, Any]) -> dict[str, Any]:
    summary = row.get("summary", {})
    for banned in ("command", "bash", "powershell", "url", "headers", "env", "prompt", "cwd"):
        if banned in summary:
            summary.pop(banned)
    return row
