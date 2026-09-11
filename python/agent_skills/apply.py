from __future__ import annotations

from fileblade_mutations import link as native_link, unlink as native_unlink

import os
from dataclasses import dataclass
from typing import Any

from fileblade_paths import NativePath

from . import discovery, registry
from .bounds import MAX_ENTRIES_PER_ROOT, bounded_names

APPLICABLE_SCOPES = ("user", "project")
ALL_AGENTS = "all"

@dataclass(frozen=True)
class Outcome:
    agent: str
    ok: bool
    changed: bool
    message: str
    touched: tuple[str, ...] = ()

    def row(self) -> dict[str, Any]:
        return {
            "agent": self.agent,
            "ok": self.ok,
            "changed": self.changed,
            "message": self.message,
            "touched": [NativePath(path) for path in self.touched],
        }

def refusal(agent: str, message: str) -> Outcome:
    return Outcome(agent, False, False, message)

def requested_agents(requested: list[str]) -> list[str]:
    if ALL_AGENTS in requested:
        return list(registry.agents())
    ordered: list[str] = []
    for agent in requested:
        if agent not in ordered:
            ordered.append(agent)
    return ordered

def find_candidate(collected: dict[tuple[str, str], discovery.Candidate], row_id: str) -> discovery.Candidate | None:
    for candidate in collected.values():
        if discovery.stable_id(candidate.scope, candidate.target) == row_id:
            return candidate
    return None

def skill_name(candidate: discovery.Candidate) -> str:
    return os.path.basename(os.path.dirname(candidate.path))

def skill_directory(candidate: discovery.Candidate) -> str:
    return os.path.realpath(os.path.dirname(candidate.path))

def valid_name(name: str) -> bool:
    return bool(name) and name not in (".", "..") and os.sep not in name and "\x00" not in name

def inside(root: str, path: str) -> bool:
    return os.path.realpath(os.path.dirname(path)) == os.path.realpath(root)

def scoped_roots(env: discovery.Environment, scope: str) -> list[tuple[registry.Root, str]]:
    return [(entry, path) for entry, path in discovery.resolve_roots(env) if entry.kind == scope]

def agent_roots(env: discovery.Environment, agent: str, scope: str) -> list[str]:
    seen: list[str] = []
    for entry, path in scoped_roots(env, scope):
        if entry.agent == agent and path not in seen:
            seen.append(path)
    return seen

def resolve_root(env: discovery.Environment, project_root: str, entry: registry.Root) -> str:
    if entry.anchor == "home":
        return os.path.join(env.home, entry.path)
    return os.path.join(project_root, entry.path)

def primary_root(env: discovery.Environment, project_root: str, agent: str, scope: str) -> str:
    for entry in registry.ROOTS:
        if entry.agent == agent and entry.kind == scope:
            return resolve_root(env, project_root, entry)
    return ""

def readers(env: discovery.Environment, scope: str, roots: list[str]) -> list[str]:
    found = {entry.agent for entry, path in scoped_roots(env, scope) if path in roots}
    return [agent for agent in registry.agents() if agent in found]

def shared_note(env: discovery.Environment, agent: str, scope: str, roots: list[str]) -> str:
    others = [reader for reader in readers(env, scope, roots) if reader != agent]
    return f"; also applies to {', '.join(others)}" if others else ""

def entries_for(root: str, target: str) -> list[str]:
    names, truncated = bounded_names(root, MAX_ENTRIES_PER_ROOT)
    if truncated:
        raise ValueError("skill root exceeds the entry limit; nothing was changed")
    found: list[str] = []
    for name in names:
        child = os.path.join(root, name)
        try:
            if os.path.realpath(child) == target:
                found.append(child)
        except OSError:
            continue
    return found

def link_on(env: discovery.Environment, agent: str, scope: str, root: str, name: str, target: str) -> Outcome:
    link = os.path.join(root, name)
    note = shared_note(env, agent, scope, [root])
    try:
        os.makedirs(root, exist_ok=True)
    except OSError as exc:
        return refusal(agent, f"cannot create {root}: {exc.strerror or exc}")
    if not inside(root, link):
        return refusal(agent, f"{link} would escape {root}")
    if os.path.lexists(link):
        if not os.path.islink(link):
            return refusal(agent, f"{link} exists and is not a symlink; nothing was changed")
        if os.path.realpath(link) == target:
            return Outcome(agent, True, False, f"{link} already points at {target}{note}")
        return refusal(agent, f"{link} is a symlink to {os.path.realpath(link)}, not to this skill; nothing was changed")
    try:
        native_link(link, target)
    except OSError as exc:
        return refusal(agent, f"cannot link {link}: {exc.strerror or exc}")
    return Outcome(agent, True, True, f"linked {link} -> {target}{note}", (link,))

def unlink_off(env: discovery.Environment, agent: str, scope: str, roots: list[str], target: str) -> Outcome:
    try:
        found = [(root, entry) for root in roots for entry in entries_for(root, target)]
    except ValueError as exc:
        return refusal(agent, str(exc))
    if not found:
        return Outcome(agent, True, False, f"no entry for {agent} in its {scope} roots")
    real = [entry for _, entry in found if not os.path.islink(entry)]
    if real:
        return refusal(agent, f"{real[0]} is a real directory, the only copy of this skill; refusing to delete it")
    note = shared_note(env, agent, scope, sorted({root for root, _ in found}))
    removed: list[str] = []
    for _, entry in found:
        try:
            native_unlink(entry, target)
        except OSError as exc:
            return Outcome(agent, False, bool(removed), f"cannot unlink {entry}: {exc.strerror or exc}", tuple(removed))
        removed.append(entry)
    return Outcome(agent, True, True, f"unlinked {', '.join(removed)}{note}", tuple(removed))

def outcome(env: discovery.Environment, project_root: str, candidate: discovery.Candidate | None,
            agent: str, state: str) -> Outcome:
    if agent not in registry.AGENT_LABELS:
        return refusal(agent, f"unknown agent id {agent}")
    if candidate is None:
        return refusal(agent, "unknown row id")
    scope = candidate.scope
    if scope not in APPLICABLE_SCOPES:
        return refusal(agent, f"{scope} rows cannot be applied; only user and project rows can")
    if scope == "project" and not project_root:
        return refusal(agent, "no project directory resolved")
    name = skill_name(candidate)
    if not valid_name(name):
        return refusal(agent, f"skill directory name {name!r} is not a single path component")
    target = skill_directory(candidate)
    roots = agent_roots(env, agent, scope)
    if not roots:
        return refusal(agent, f"{agent} documents no {scope} skill root")
    if state == "on":
        if agent in candidate.agents:
            return Outcome(agent, True, False, f"already applied to {agent}")
        return link_on(env, agent, scope, primary_root(env, project_root, agent, scope), name, target)
    if agent not in candidate.agents:
        return Outcome(agent, True, False, f"not applied to {agent}")
    return unlink_off(env, agent, scope, roots, target)

def apply(env: discovery.Environment, row_id: str, requested: list[str], state: str) -> dict[str, Any]:
    project_root, _ = discovery.chain_for(env)
    candidate = find_candidate(discovery.candidates(env), row_id)
    results: list[Outcome] = []
    for agent in requested_agents(requested):
        if results and results[-1].changed:
            candidate = find_candidate(discovery.candidates(env), row_id)
        results.append(outcome(env, project_root, candidate, agent, state))
    return {
        "ok": all(result.ok for result in results),
        "schemaVersion": 1,
        "project": NativePath(project_root),
        "id": row_id,
        "state": state,
        "results": [result.row() for result in results],
    }
