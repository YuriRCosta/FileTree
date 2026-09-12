from __future__ import annotations

import hashlib
import json
import os
import stat
import sys
import tomllib
from dataclasses import dataclass, field
from typing import Any

from fileblade_paths import NativePath, display
from fileblade_inventory import lane_rows, watch_path

from . import frontmatter, registry
from .bounds import (
    MAX_CONFIG_BYTES,
    MAX_EXTENSIONS,
    MAX_MANIFEST_BYTES,
    MAX_DETAIL,
    MAX_ENTRIES_PER_ROOT,
    MAX_ITEMS,
    MAX_NAME,
    MAX_PATH,
    MAX_PLUGINS,
    MAX_PROJECT_WALK,
    MAX_ROOTS,
    MAX_TOTAL_ENTRIES,
    clean,
    artifact_metrics,
    bounded_names,
    env_path_value,
    read_document,
    read_secure_document,
)

SKILL_FILE = "SKILL.md"

@dataclass
class Environment:
    home: str
    anchor: str = ""
    exact: bool = False
    platform: str = sys.platform
    prefix: str = ""
    environ: dict[str, str] = field(default_factory=dict)
    enforce_secure_system: bool = True
    scope: str = "all"

@dataclass
class Budget:
    entries: int = 0
    truncated: bool = False

    def spend(self, amount: int = 1) -> bool:
        if self.entries + amount > MAX_TOTAL_ENTRIES:
            self.truncated = True
            return False
        self.entries += amount
        return True

@dataclass
class Candidate:
    path: str
    target: str
    scope: str
    source: str
    agents: set[str] = field(default_factory=set)
    roots: set[str] = field(default_factory=set)
    precedence: int | None = None
    order: int = 1 << 30
    enabled: bool | None = None

def under(prefix: str, path: str) -> str:
    if not prefix:
        return path
    return os.path.join(prefix, path.lstrip("/\\"))

def chain_for(env: Environment) -> tuple[str, list[str]]:
    if env.scope == "user":
        return "", []
    return exact_chain(env.anchor) if env.exact else project_chain(env.anchor)

def exact_chain(anchor: str) -> tuple[str, list[str]]:
    if not anchor:
        return "", []
    root = os.path.abspath(os.path.expanduser(anchor))
    if os.path.isfile(root):
        root = os.path.dirname(root)
    return root, [root]

def project_chain(anchor: str) -> tuple[str, list[str]]:
    if not anchor:
        return "", []
    start = os.path.abspath(os.path.expanduser(anchor))
    if os.path.isfile(start):
        start = os.path.dirname(start)
    chain: list[str] = []
    root = ""
    current = start
    for _ in range(MAX_PROJECT_WALK):
        chain.append(current)
        if any(os.path.exists(os.path.join(current, marker)) for marker in registry.PROJECT_MARKERS):
            root = current
            break
        parent = os.path.dirname(current)
        if parent == current:
            break
        current = parent
    if root:
        return root, chain
    return start, [start]

def resolve_roots(env: Environment) -> list[tuple[registry.Root, str]]:
    project_root, chain = chain_for(env)
    resolved: list[tuple[registry.Root, str]] = []
    for entry in registry.ROOTS:
        if len(resolved) >= MAX_ROOTS:
            break
        if entry.anchor != "walk" and env.scope == "project":
            continue
        if entry.anchor == "home":
            resolved.append((entry, os.path.join(env.home, entry.path)))
        elif entry.anchor == "absolute":
            resolved.append((entry, under(env.prefix, entry.path)))
        elif entry.anchor == "managed-claude":
            managed = registry.MANAGED_CLAUDE.get(env.platform)
            if managed:
                resolved.append((entry, under(env.prefix, managed)))
        elif entry.anchor == "walk":
            for directory in chain:
                if len(resolved) >= MAX_ROOTS:
                    break
                resolved.append((entry, os.path.join(directory, entry.path)))
    return resolved

def load_json(path: str) -> Any:
    text = read_document(path, MAX_CONFIG_BYTES)
    if not text:
        return None
    try:
        return json.loads(text)
    except (ValueError, RecursionError):
        return None

def claude_plugins(env: Environment) -> list[tuple[str, str, bool | None]]:
    installed = load_json(os.path.join(env.home, ".claude", "plugins", "installed_plugins.json"))
    settings = load_json(os.path.join(env.home, ".claude", "settings.json"))
    enabled_map = {}
    if isinstance(settings, dict) and isinstance(settings.get("enabledPlugins"), dict):
        enabled_map = settings["enabledPlugins"]
    plugins = installed.get("plugins") if isinstance(installed, dict) else None
    if not isinstance(plugins, dict):
        return []
    found: list[tuple[str, str, bool | None]] = []
    for key in sorted(plugins):
        if len(found) >= MAX_PLUGINS:
            break
        entries = plugins[key]
        if not isinstance(entries, list) or not entries or not isinstance(entries[0], dict):
            continue
        install_path = entries[0].get("installPath")
        if not isinstance(install_path, str) or not install_path:
            continue
        state = enabled_map.get(key)
        found.append((key, install_path, state if isinstance(state, bool) else None))
    return found

def codex_switches(env: Environment) -> dict[str, bool]:
    text = read_document(os.path.join(env.home, ".codex", "config.toml"), MAX_CONFIG_BYTES)
    if not text:
        return {}
    try:
        parsed = tomllib.loads(text)
    except (tomllib.TOMLDecodeError, ValueError, RecursionError):
        return {}
    section = parsed.get("skills")
    rows = section.get("config") if isinstance(section, dict) else None
    switches: dict[str, bool] = {}
    if not isinstance(rows, list):
        return switches
    for row in rows[:MAX_PLUGINS]:
        if not isinstance(row, dict):
            continue
        path = row.get("path")
        state = row.get("enabled")
        if isinstance(path, str) and isinstance(state, bool):
            switches[os.path.realpath(os.path.expanduser(path))] = state
    return switches

def stable_id(scope: str, target: str) -> str:
    digest = hashlib.sha256(f"{scope}\0{target}".encode("utf-8", "surrogateescape"))
    return digest.hexdigest()[:16]

def descriptor(skill_dir: str) -> str | None:
    watch_path(skill_dir, directory=True)
    candidate = os.path.join(skill_dir, SKILL_FILE)
    try:
        if not os.path.isfile(candidate):
            return None
    except OSError:
        return None
    return candidate

def classify(target: str, env: Environment, source: str) -> str:
    if source:
        return source
    system = under(env.prefix, "/usr/share")
    if target == system or target.startswith(system + os.sep):
        return "system"
    return "loose"

def add_candidate(
    collected: dict[tuple[str, str], Candidate],
    entry: registry.Root,
    root_path: str,
    skill_dir: str,
    env: Environment,
    order: int,
    source: str,
    enabled: bool | None,
) -> None:
    path = descriptor(skill_dir)
    if path is None or len(path) > MAX_PATH:
        return
    target = os.path.realpath(path)
    scope = entry.kind if entry.kind != "plugin" else "plugin"
    if source.startswith("plugin:"):
        scope = "plugin"
    key = (scope, target)
    existing = collected.get(key)
    if existing is None:
        existing = Candidate(
            path=path,
            target=target,
            scope=scope,
            source=classify(target, env, source),
            precedence=entry.precedence,
            order=order,
            enabled=enabled,
        )
        collected[key] = existing
    elif (entry.precedence is not None and existing.precedence is None) or order < existing.order:
        existing.path = path
        existing.precedence = entry.precedence
        existing.order = order
    existing.agents.add(entry.agent)
    existing.roots.add(root_path)
    if enabled is not None:
        existing.enabled = enabled

def scan_root(
    collected: dict[tuple[str, str], Candidate],
    entry: registry.Root,
    root_path: str,
    env: Environment,
    budget: Budget,
    order: int,
    source: str = "",
    enabled: bool | None = None,
) -> None:
    names, truncated = bounded_names(root_path, min(MAX_ENTRIES_PER_ROOT, MAX_TOTAL_ENTRIES - budget.entries))
    budget.truncated |= truncated
    reserved_allowed = entry.agent == "claude-code" and entry.kind in ("managed", "user", "project")
    seen = 0
    for name in names:
        if seen >= MAX_ENTRIES_PER_ROOT or not budget.spend():
            budget.truncated = True
            return
        child = os.path.join(root_path, name)
        try:
            if not os.path.isdir(child):
                continue
        except OSError:
            continue
        seen += 1
        if descriptor(child) is None:
            if reserved_allowed and name.lower() == registry.RESERVED_CLAUDE_SUBDIR:
                scan_reserved(collected, entry, child, env, budget, order)
            continue
        add_candidate(collected, entry, root_path, child, env, order, source, enabled)

def scan_reserved(
    collected: dict[tuple[str, str], Candidate],
    entry: registry.Root,
    root_path: str,
    env: Environment,
    budget: Budget,
    order: int,
) -> None:
    names, truncated = bounded_names(root_path, min(MAX_ENTRIES_PER_ROOT, MAX_TOTAL_ENTRIES - budget.entries))
    budget.truncated |= truncated
    seen = 0
    for name in names:
        if seen >= MAX_ENTRIES_PER_ROOT or not budget.spend():
            budget.truncated = True
            return
        child = os.path.join(root_path, name)
        try:
            if not os.path.isdir(child):
                continue
        except OSError:
            continue
        seen += 1
        if descriptor(child) is not None:
            add_candidate(collected, entry, root_path, child, env, order, "synced", None)

def scan_plugins(
    collected: dict[tuple[str, str], Candidate],
    env: Environment,
    budget: Budget,
    order: int,
) -> None:
    entry = registry.Root("claude-code", "plugin", "absolute", "", None, registry.DOCS["claude-code"])
    for key, install_path, enabled in claude_plugins(env):
        label = f"plugin:{key}"
        if descriptor(install_path) is not None and budget.spend():
            add_candidate(collected, entry, install_path, install_path, env, order, label, enabled)
        scan_root(collected, entry, os.path.join(install_path, "skills"), env, budget, order, label, enabled)

def emit_row(candidate: Candidate) -> dict[str, Any]:
    text = read_document(candidate.path)
    name = frontmatter.value(text, "name") or os.path.basename(os.path.dirname(candidate.path))
    agents = sorted(candidate.agents)[: len(registry.AGENT_LABELS)]
    badges = [candidate.scope]
    if len(agents) > 1:
        badges.append(f"{len(agents)} agents")
    if candidate.enabled is False:
        badges.append("disabled")
    if candidate.scope == "extension":
        badges.append("extension")
    return {
        "id": stable_id(candidate.scope, candidate.target),
        "name": clean(display(name), MAX_NAME),
        "detail": clean(frontmatter.value(text, "description"), MAX_DETAIL),
        "metrics": artifact_metrics(candidate.path, text, frontmatter.value(text, "description")),
        "scope": candidate.scope,
        "root_kind": candidate.scope,
        "source": candidate.source,
        "path": NativePath(candidate.path),
        "link_target": NativePath(candidate.target) if candidate.target != candidate.path else "",
        "agents": agents,
        "agent_labels": [registry.AGENT_LABELS[a] for a in agents if a in registry.AGENT_LABELS],
        "roots": [NativePath(root) for root in sorted(candidate.roots)[:8]],
        "precedence": candidate.precedence,
        "enabled": candidate.enabled,
        "badges": badges,
    }

SCOPE_ORDER = {"project": 0, "user": 1, "extension": 2, "plugin": 3, "managed": 4, "system": 5}
PROJECT_SCOPES = frozenset({"project"})

def candidates(env: Environment, budget: Budget | None = None) -> dict[tuple[str, str], Candidate]:
    collected: dict[tuple[str, str], Candidate] = {}
    spend = budget if budget is not None else Budget()
    for order, (entry, root_path) in enumerate(resolve_roots(env)):
        scan_root(collected, entry, root_path, env, spend, order)
    if env.scope != "project":
        scan_plugins(collected, env, spend, 1 << 20)
    return collected

def collect(env: Environment) -> dict[str, Any]:
    project_root, _ = chain_for(env)
    budget = Budget()
    collected = candidates(env, budget)
    switches = codex_switches(env)
    for candidate in collected.values():
        if candidate.enabled is None and "codex" in candidate.agents:
            candidate.enabled = switches.get(candidate.target)
    rows = lane_rows([emit_row(candidate) for candidate in collected.values()], env.scope, PROJECT_SCOPES)
    rows.sort(key=lambda row: (SCOPE_ORDER.get(row["scope"], 9), row["source"], row["name"].lower(), row["id"]))
    truncated = budget.truncated or len(rows) > MAX_ITEMS
    return {
        "ok": True,
        "schemaVersion": 1,
        "project": NativePath(project_root),
        "count": min(len(rows), MAX_ITEMS),
        "truncated": truncated,
        "agents": list(registry.agents()),
        "items": rows[:MAX_ITEMS],
    }
