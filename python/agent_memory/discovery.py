from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from fileblade_inventory import lane_rows
from fileblade_paths import NativePath, display

from .adapters import ADAPTERS, claim
from .common import (
    MAX_EXTRA_ROOTS,
    artifact_metrics,
    env_path_value,
    MAX_NAME_CHARS,
    Budget,
    bounded_document,
    descriptor_head,
    expanded,
    frontmatter_field,
    listed_files,
    project_root,
    realpath_of,
    stable_id,
    summary_line,
)

EXCLUDED_KINDS = ("session-transcript", "private-database", "configuration")
KIND_ORDER = ("instructions", "rules", "auto-memory", "system-prompt", "extra")

def config_path(home: Path, environ: dict[str, str], override: str) -> Path:
    if override:
        return expanded(override)
    base = env_path_value("XDG_CONFIG_HOME", environ)
    root = expanded(base) if base else home / ".config"
    current = root / "data-goblin.fileblade-memory" / "config.json"
    legacy = root / "kurt.agent-memory" / "config.json"
    if current.exists() or not legacy.exists():
        return current
    return legacy

def extra_roots(path: Path) -> list[dict[str, str]]:
    head = descriptor_head(path)
    if head is None:
        return []
    try:
        parsed = json.loads(head[0])
    except (ValueError, TypeError):
        return []
    if not isinstance(parsed, dict):
        return []
    entries = parsed.get("extraRoots")
    if not isinstance(entries, list):
        return []
    roots: list[dict[str, str]] = []
    for entry in entries[:MAX_EXTRA_ROOTS]:
        if isinstance(entry, str):
            roots.append({"path": entry, "label": ""})
        elif isinstance(entry, dict) and isinstance(entry.get("path"), str):
            label = entry.get("label")
            roots.append({"path": entry["path"], "label": label if isinstance(label, str) else ""})
    return roots

def extra_claims(roots: list[dict[str, str]], budget: Budget) -> list[dict[str, Any]]:
    claims: list[dict[str, Any]] = []
    for entry in roots:
        if not budget.take_source():
            break
        target = expanded(entry["path"])
        label = entry["label"][:MAX_NAME_CHARS]
        try:
            files = [target] if target.is_file() else listed_files(target)
        except OSError:
            continue
        for candidate in files:
            row = claim(candidate, "user", "extra", "extra", 90, label)
            row["discovery"] = "user-configured"
            claims.append(row)
    return claims

def describe(path: Path) -> dict[str, Any] | None:
    head = descriptor_head(path)
    if head is None:
        return None
    text, size = head
    return {
        "detail": frontmatter_field(text, "description") or summary_line(text),
        "bytes": size,
        "memoryType": frontmatter_field(text, "type"),
        "paths": frontmatter_field(text, "paths"),
        "modified": frontmatter_field(text, "modified"),
        "metrics": artifact_metrics(path, text, size),
    }

def new_row(path: Path, target: str, entry: dict[str, Any], described: dict[str, Any], reader: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": stable_id(target),
        "name": display(path.name)[:MAX_NAME_CHARS],
        "path": NativePath(str(path)),
        "realpath": NativePath(target),
        "alias": str(path) != target,
        "aliases": [] if str(path) == target else [NativePath(str(path))],
        "kind": entry["kind"],
        "scope": entry["scope"],
        "note": str(entry["note"])[:MAX_NAME_CHARS],
        "readers": [reader],
        "excluded": bool(entry.get("excluded")),
        "inlineBytes": entry.get("inline"),
        "detail": described["detail"],
        "bytes": described["bytes"],
        "memoryType": described["memoryType"],
        "activation": "path-scoped" if described["paths"] else "always",
        "modified": described["modified"],
        "metrics": described["metrics"],
        "badges": [],
    }

def merge(claims: list[dict[str, Any]], budget: Budget) -> list[dict[str, Any]]:
    rows: dict[str, dict[str, Any]] = {}
    for entry in claims:
        path: Path = entry["path"]
        target = realpath_of(path)
        reader = {
            "agent": entry["agent"],
            "loadOrder": entry["loadOrder"],
            "scope": entry["scope"],
            "kind": entry["kind"],
            "discovery": entry["discovery"],
            "excluded": bool(entry.get("excluded")),
        }
        existing = rows.get(target)
        if existing is None:
            if not budget.take_item():
                break
            described = describe(path)
            if described is None:
                continue
            built = new_row(path, target, entry, described, reader)
            if built["inlineBytes"] is not None:
                built["detail"] = ""
                built["bytes"] = int(built["inlineBytes"])
                built["memoryType"] = ""
                built["metrics"] = dict(built["metrics"])
                built["metrics"].update({
                    "bytes": built["bytes"],
                    "characters": None,
                    "words": None,
                    "tokens": None,
                })
            rows[target] = built
            continue
        if str(path) != existing["path"] and str(path) not in existing["aliases"]:
            existing["aliases"].append(NativePath(str(path)))
        if not any(item["agent"] == reader["agent"] for item in existing["readers"]):
            existing["readers"].append(reader)
        if KIND_ORDER.index(entry["kind"]) < KIND_ORDER.index(existing["kind"]):
            existing["kind"] = entry["kind"]
    for row in rows.values():
        row["readers"].sort(key=lambda item: (item["agent"], item["loadOrder"]))
        if len(row["readers"]) > 1:
            row["badges"].append("shared")
        flagged = [reader for reader in row["readers"] if reader["excluded"]]
        row["excluded"] = bool(flagged) and len(flagged) == len(row["readers"])
        if row["excluded"]:
            row["badges"].append("excluded")
        elif flagged:
            row["badges"].append("partly-excluded")
        if row["inlineBytes"] is not None:
            row["badges"].append("inline")
        if row["alias"]:
            row["badges"].append("linked")
        if row["kind"] == "auto-memory":
            row["badges"].append("agent-written")
    return list(rows.values())

PROJECT_SCOPES = frozenset({"project", "local"})

def build_context(project: str, home: str = "", environ: dict[str, str] | None = None,
                  prefix: str = "", enforce_secure_system: bool = True, exact: bool = False,
                  scope: str = "all") -> dict[str, Any]:
    variables = dict(environ if environ is not None else os.environ)
    home_path = expanded(home) if home else Path(os.path.expanduser("~"))
    start = expanded(project) if project else home_path
    root = str(start if start.is_dir() else start.parent) if exact and project else project_root(str(start), home_path)
    if scope == "user":
        root = ""
    anchor = start
    try:
        if anchor.is_file():
            anchor = anchor.parent
    except OSError:
        anchor = start
    return {
        "home": home_path,
        "cwd": expanded(root) if root else start,
        "anchor": anchor,
        "projectRoot": NativePath(root),
        "environ": variables,
        "prefix": str(expanded(prefix)) if prefix else "",
        "enforceSecureSystem": enforce_secure_system,
        "scope": scope,
    }

def collect_in(context: dict[str, Any], config: str = "") -> dict[str, Any]:
    budget = Budget()
    home_path: Path = context["home"]
    claims: list[dict[str, Any]] = []
    for _, adapter in ADAPTERS:
        try:
            claims.extend(adapter(context, budget))
        except OSError:
            continue
    if context.get("scope", "all") != "project":
        claims.extend(extra_claims(extra_roots(config_path(home_path, context["environ"], config)), budget))
    items = lane_rows(merge(claims, budget), context.get("scope", "all"), PROJECT_SCOPES)
    items.sort(key=lambda row: (KIND_ORDER.index(row["kind"]), row["scope"], row["name"].lower()))
    return bounded_document({
        "ok": True,
        "project": context["projectRoot"],
        "home": NativePath(str(home_path)),
        "count": len(items),
        "excludedKinds": list(EXCLUDED_KINDS),
        "items": items,
    }, budget)

def collect(project: str, home: str = "", config: str = "", environ: dict[str, str] | None = None,
            prefix: str = "", enforce_secure_system: bool = True, exact: bool = False,
            scope: str = "all") -> dict[str, Any]:
    return collect_in(build_context(project, home, environ, prefix, enforce_secure_system, exact, scope), config)
