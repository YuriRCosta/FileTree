from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from fileblade_inventory import lane_rows
from fileblade_paths import NativePath

from .adapters import ADAPTERS
from .events import canonical
from .labels import attach_labels, labels_path, load_labels
from .redaction import assert_safe
from .safeio import Budget, expanded, is_plain_dir

SCHEMA_VERSION = 1
PROJECT_MARKERS = (".git", ".claude", ".agents", ".codex", ".opencode", ".pi", ".github")
EVENT_ORDER = ("documented", "undocumented-event", "code-hosted")

def project_root(start: str, home: Path) -> str:
    if not start:
        return ""
    current = expanded(start)
    if not is_plain_dir(current):
        current = current.parent
    chain = [current, *current.parents][:24]
    for candidate in chain:
        if candidate == home.parent:
            break
        for marker in PROJECT_MARKERS:
            if (candidate / marker).exists():
                return str(candidate)
    return ""

def sharing_key(entry: dict[str, Any]) -> tuple[str, str]:
    return (str(entry["summary"].get("digest", "")), canonical(entry["agent"], entry["event"]))

def annotate_applied_agents(rows: list[dict[str, Any]]) -> None:
    groups: dict[tuple[str, str], set[str]] = {}
    for entry in rows:
        key = sharing_key(entry)
        if key[0]:
            groups.setdefault(key, set()).add(entry["agent"])
    for entry in rows:
        key = sharing_key(entry)
        entry["appliedAgents"] = sorted(groups[key]) if key[0] else [entry["agent"]]

PROJECT_SCOPES = frozenset({"project", "local"})

def collect(project: str, home: str = "", environ: dict[str, str] | None = None,
            etc_root: str = "/etc", policy_owner_uid: int = 0, exact: bool = False,
            scope: str = "all", budget: Budget | None = None) -> dict[str, Any]:
    variables = dict(environ if environ is not None else os.environ)
    home_path = expanded(home) if home else Path(os.path.expanduser("~"))
    if scope == "user":
        root = NativePath("")
    else:
        root = NativePath(str(expanded(project)) if exact and project else project_root(project or str(home_path), home_path))
    budget = Budget() if budget is None else budget
    context = {
        "home": home_path,
        "projectRoot": root,
        "environ": variables,
        "etcRoot": expanded(etc_root),
        "policyOwnerUid": int(policy_owner_uid),
        "scope": scope,
    }
    rows: list[dict[str, Any]] = []
    agents: dict[str, str] = {}
    for name, adapter in ADAPTERS:
        try:
            produced = adapter(context, budget)
        except OSError:
            produced = []
        agents[name] = "code-hosted" if name in ("opencode", "pi") else "documented"
        rows.extend(assert_safe(entry) for entry in produced)
    rows = lane_rows(rows, scope, PROJECT_SCOPES)
    annotate_applied_agents(rows)
    attach_labels(rows, load_labels(labels_path(home_path, variables)))
    rows.sort(key=lambda entry: (
        EVENT_ORDER.index(entry["supportStatus"]) if entry["supportStatus"] in EVENT_ORDER else 9,
        entry["agent"],
        entry["event"],
        entry["source"]["path"],
        entry["index"],
    ))
    return {
        "schemaVersion": SCHEMA_VERSION,
        "ok": True,
        "project": root,
        "home": NativePath(str(home_path)),
        "count": len(rows),
        "truncated": budget.truncated,
        "agents": agents,
        "items": rows,
    }
