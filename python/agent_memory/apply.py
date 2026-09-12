from __future__ import annotations

from fileblade_mutations import link as native_link, unlink as native_unlink

import os
import stat
from pathlib import Path
from typing import Any

from fileblade_paths import NativePath

from . import discovery
from .adapters import ADAPTERS
from .common import SCHEMA_VERSION, ancestors_of, env_path, expanded, realpath_of, safe_basename

Context = dict[str, Any]
Result = dict[str, Any]

AGENT_IDS = tuple(name for name, _ in ADAPTERS)
LINKABLE_KIND = "instructions"
USER_SCOPES = ("user",)
PROJECT_SCOPES = ("project", "local")
STATES = ("on", "off")
PROJECT_NAMES = {
    "claude-code": "CLAUDE.md",
    "codex": "AGENTS.md",
    "opencode": "AGENTS.md",
    "pi": "AGENTS.md",
    "copilot-cli": "AGENTS.md",
}
ANTIGRAVITY_RULES = Path(".agents") / "rules"

def result(agent: str, ok: bool, changed: bool, message: str, touched: list[str] | None = None) -> Result:
    return {"agent": agent, "ok": ok, "changed": changed, "message": message,
            "touched": [NativePath(path) for path in touched or []]}

def refused(agent: str, message: str) -> Result:
    return result(agent, False, False, message)

def unchanged(agent: str, message: str) -> Result:
    return result(agent, True, False, message)

def user_directory(agent: str, context: Context) -> Path | None:
    home: Path = context["home"]
    environ = context["environ"]
    if agent == "claude-code":
        return home / ".claude"
    if agent == "codex":
        return env_path("CODEX_HOME", environ) or home / ".codex"
    if agent == "opencode":
        return (env_path("XDG_CONFIG_HOME", environ) or home / ".config") / "opencode"
    if agent == "pi":
        return home / ".pi" / "agent"
    if agent == "copilot-cli":
        return env_path("COPILOT_HOME", environ) or home / ".copilot"
    if agent == "antigravity":
        return home / ".gemini"
    return None

def user_name(agent: str, context: Context) -> str:
    if agent == "copilot-cli":
        return "copilot-instructions.md"
    if agent == "antigravity":
        return "GEMINI.md"
    return PROJECT_NAMES.get(agent, "")

def project_base(context: Context) -> Path:
    root = context["projectRoot"]
    return expanded(root) if root else context["anchor"]

def chain_directories(context: Context) -> list[Path]:
    anchor: Path = context["anchor"]
    return ancestors_of(anchor, project_base(context))

def same_place(left: Path, right: Path) -> bool:
    return realpath_of(left) == realpath_of(right)

def contained(path: Path, base: Path) -> bool:
    inner = realpath_of(path)
    outer = realpath_of(base)
    return inner == outer or inner.startswith(outer.rstrip("/") + "/")

def user_target(agent: str, context: Context) -> Path | Result:
    directory = user_directory(agent, context)
    name = user_name(agent, context)
    if directory is None:
        return refused(agent, f"unknown agent id {agent!r}")
    if not name:
        return refused(agent, "Gemini CLI context.fileName is configured empty, so there is no file name to link")
    return directory / name

def project_target(agent: str, context: Context, row_path: Path, source: Path) -> Path | Result:
    base = project_base(context)
    if agent == "antigravity":
        if source.suffix != ".md":
            return refused(agent, f"Antigravity reads only .md files under {ANTIGRAVITY_RULES}")
        return base / ANTIGRAVITY_RULES / source.name
    name = PROJECT_NAMES.get(agent, "")
    if not name:
        return refused(agent, f"unknown agent id {agent!r}")
    directory = row_path.parent
    if not any(same_place(directory, candidate) for candidate in chain_directories(context)):
        return refused(agent, f"{agent} reads {name} only between {base} and {context['anchor']}, not in {directory}")
    return directory / name

def resolve_target(agent: str, context: Context, row: dict[str, Any]) -> Path | Result:
    if agent not in AGENT_IDS:
        return refused(agent, f"unknown agent id {agent!r}")
    kind = str(row.get("kind", ""))
    scope = str(row.get("scope", ""))
    if kind != LINKABLE_KIND:
        return refused(agent, f"{row.get('name', '')} is a {kind} file; {agent} has no documented equivalent for it")
    if scope in USER_SCOPES:
        target = user_target(agent, context)
        base = user_directory(agent, context)
    elif scope in PROJECT_SCOPES:
        target = project_target(agent, context, Path(str(row["path"])), Path(str(row["realpath"])))
        base = project_base(context)
    else:
        return refused(agent, f"{scope} scope has no documented location for {agent}")
    if isinstance(target, dict):
        return target
    if base is None or safe_basename(target.name) != target.name or not contained(target.parent, base):
        return refused(agent, f"{target} would escape {base}")
    return target

def inspect(path: Path) -> os.stat_result | None:
    try:
        return os.lstat(path)
    except FileNotFoundError:
        return None

def turn_on(agent: str, target: Path, source: Path) -> Result:
    try:
        current = inspect(target)
    except OSError as error:
        return refused(agent, f"cannot inspect {target}: {error.strerror or error}")
    if current is not None:
        if stat.S_ISLNK(current.st_mode):
            if same_place(target, source):
                return unchanged(agent, f"{target} already links to this file")
            return refused(agent, f"{target} already links to a different file ({os.readlink(target)})")
        if same_place(target, source):
            return unchanged(agent, f"{agent} already reads {target}")
        return refused(agent, f"{target} exists as a real file; not replacing it")
    try:
        native_link(target, source)
    except FileExistsError:
        return refused(agent, f"{target} appeared while linking; nothing changed")
    except OSError as error:
        return refused(agent, f"cannot link {target}: {error.strerror or error}")
    return result(agent, True, True, f"linked {target}", [str(target)])

def turn_off(agent: str, target: Path, source: Path, reads_directly: bool) -> Result:
    try:
        current = inspect(target)
    except OSError as error:
        return refused(agent, f"cannot inspect {target}: {error.strerror or error}")
    if current is None:
        if reads_directly:
            return unchanged(agent, f"{agent} reads this file without a link; nothing to remove")
        return unchanged(agent, f"nothing linked at {target}")
    if not stat.S_ISLNK(current.st_mode):
        if same_place(target, source):
            return unchanged(agent, f"{target} is the memory file itself; a real file is never deleted")
        return unchanged(agent, f"{target} is a separate real file; left alone")
    if not same_place(target, source):
        return unchanged(agent, f"{target} links to a different file; left alone")
    try:
        native_unlink(target, source, current)
    except OSError as error:
        return refused(agent, f"cannot unlink {target}: {error.strerror or error}")
    return result(agent, True, True, f"unlinked {target}", [str(target)])

def find_row(document: dict[str, Any], row_id: str) -> dict[str, Any] | None:
    for row in document.get("items", []):
        if str(row.get("id")) == row_id:
            return row
    return None

def document(context: Context, results: list[Result]) -> dict[str, Any]:
    failures = [f"{entry['agent']}: {entry['message']}" for entry in results if not entry["ok"]]
    return {
        "ok": not failures,
        "schemaVersion": SCHEMA_VERSION,
        "project": context["projectRoot"],
        "message": "; ".join(failures),
        "results": results,
    }

def apply_in(context: Context, config: str, row_id: str, agents: list[str], state: str) -> dict[str, Any]:
    if state not in STATES:
        return document(context, [refused(agent, f"state must be one of {', '.join(STATES)}") for agent in agents])
    if not agents:
        return document(context, [refused("", "at least one --agent is required")])
    row = find_row(discovery.collect_in(context, config), row_id)
    if row is None:
        where = context["projectRoot"] or str(context["anchor"])
        return document(context, [refused(agent, f"no memory row with id {row_id!r} for {where}") for agent in agents])
    source = Path(str(row["realpath"]))
    if not source.is_file():
        return document(context, [refused(agent, f"{source} is not a regular file") for agent in agents])
    readers = {str(reader.get("agent")) for reader in row.get("readers", [])}
    results: list[Result] = []
    for agent in agents:
        target = resolve_target(agent, context, row)
        if isinstance(target, dict):
            results.append(target)
        elif state == "on":
            results.append(turn_on(agent, target, source))
        else:
            results.append(turn_off(agent, target, source, agent in readers))
    return document(context, results)

def apply(project: str, row_id: str, agents: list[str], state: str, home: str = "", config: str = "",
          environ: dict[str, str] | None = None, prefix: str = "",
          enforce_secure_system: bool = True, exact: bool = False) -> dict[str, Any]:
    context = discovery.build_context(project, home, environ, prefix, enforce_secure_system, exact)
    return apply_in(context, config, row_id, agents, state)
