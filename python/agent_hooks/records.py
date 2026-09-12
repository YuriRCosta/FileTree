from __future__ import annotations

from copy import deepcopy
import hashlib
import json
from typing import Any

from .safeio import MAX_ITEMS_PER_SOURCE, stable_id


def fingerprint(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(",", ":"))
    return hashlib.sha256(encoded.encode("ascii")).hexdigest()


def group_fields(group: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in group.items() if key != "hooks"}


def identity(agent: str, path: str, event: str, index: int, group: dict[str, Any],
             entry: dict[str, Any], namespace: str = "") -> str:
    content = {"group": group_fields(group), "entry": entry,
               "grouped": isinstance(group.get("hooks"), list)}
    return stable_id(agent, path, event, index, namespace, fingerprint(content))


def mapping(row: dict[str, Any], document: dict[str, Any]) -> dict[str, Any] | None:
    key = row.get("group", "") if row["agent"] == "antigravity" else "hooks"
    value = document.get(key)
    return value if isinstance(value, dict) else None


def locate(row: dict[str, Any], document: dict[str, Any]) -> tuple | None:
    container = mapping(row, document)
    definitions = container.get(row["event"]) if container is not None else None
    index = row.get("index")
    if not isinstance(definitions, list) or type(index) is not int or index < 0:
        return None
    group_index, entry_index = divmod(index, MAX_ITEMS_PER_SOURCE)
    if group_index >= len(definitions) or not isinstance(definitions[group_index], dict):
        return None
    group = definitions[group_index]
    entries = group.get("hooks")
    candidates = entries if isinstance(entries, list) else [group]
    if entry_index >= len(candidates) or not isinstance(candidates[entry_index], dict):
        return None
    return definitions, group_index, entry_index, group, candidates[entry_index]


def matches(row: dict[str, Any], document: dict[str, Any]) -> bool:
    found = locate(row, document)
    return found is not None and row["id"] == identity(
        row["agent"], str(row["source"]["path"]), row["event"], row["index"],
        found[3], found[4], str(row.get("group", "")))


def detach(row: dict[str, Any], document: dict[str, Any]) -> dict[str, Any]:
    definitions, group_index, entry_index, group, entry = locate(row, document)
    grouped = isinstance(group.get("hooks"), list)
    removed_group = not grouped or len(group["hooks"]) == 1
    record = {"format": 2, "agent": row["agent"], "event": row["event"],
              "group": str(row.get("group", "")), "index": row["index"],
              "grouped": grouped, "removedGroup": removed_group,
              "fields": deepcopy(group_fields(group)) if grouped else {},
              "entry": deepcopy(entry), "before": fingerprint(definitions)}
    if removed_group:
        definitions.pop(group_index)
    else:
        group["hooks"].pop(entry_index)
    if not definitions:
        mapping(row, document).pop(row["event"])
    record["after"] = fingerprint(definitions or None)
    return record


def attach(record: dict[str, Any], document: dict[str, Any]) -> bool:
    if (record.get("format") != 2 or type(record.get("index")) is not int
            or not 0 <= record["index"] < MAX_ITEMS_PER_SOURCE ** 2
            or type(record.get("grouped")) is not bool or type(record.get("removedGroup")) is not bool
            or not isinstance(record.get("fields"), dict) or "hooks" in record["fields"]
            or not isinstance(record.get("entry"), dict) or not isinstance(record.get("group"), str)
            or any(not isinstance(record.get(key), str) or len(record[key]) != 64 for key in ("before", "after"))):
        raise ValueError("restore record is incomplete")
    container = mapping(record, document)
    if container is None:
        raise ValueError("the source hook container changed; restore it manually")
    current = container.get(record["event"])
    current_id = fingerprint(current)
    if current_id == record["before"]:
        return False
    if current_id != record["after"] or current is not None and not isinstance(current, list):
        raise ValueError("the source event changed since removal; restore it manually")
    definitions = deepcopy(current) if current is not None else []
    group_index, entry_index = divmod(record["index"], MAX_ITEMS_PER_SOURCE)
    if record["removedGroup"]:
        if group_index > len(definitions) or record["grouped"] and entry_index != 0:
            raise ValueError("restore record has an invalid group position")
        group = dict(record["fields"], hooks=[record["entry"]]) if record["grouped"] else record["entry"]
        definitions.insert(group_index, deepcopy(group))
    else:
        group = definitions[group_index] if group_index < len(definitions) else None
        entries = group.get("hooks") if isinstance(group, dict) else None
        if not record["grouped"] or not isinstance(entries, list) or entry_index > len(entries):
            raise ValueError("restore record has an invalid entry position")
        entries.insert(entry_index, deepcopy(record["entry"]))
    if fingerprint(definitions) != record["before"]:
        raise ValueError("restore record does not match the original event")
    container[record["event"]] = definitions
    return True
