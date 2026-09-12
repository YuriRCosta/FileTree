from __future__ import annotations

from fileblade_inventory import watch_path

from pathlib import Path
from typing import Any, Callable

from .common import (
    DEFAULT_BOUNDARY_MARKERS,
    DEFAULT_DISCOVERY_MAX_DIRS,
    IGNORED_DISCOVERY_DIRS,
    MAX_CONTEXT_NAMES,
    MAX_DISCOVERY_DIRS,
    MAX_ENV_DIRS,
    MAX_EXTENSIONS,
    MAX_MANIFEST_BYTES,
    MAX_SOURCES,
    bounded_directories,
    MAX_FALLBACK_NAMES,
    MAX_INSTRUCTION_ENTRIES,
    Budget,
    ancestors_of,
    bounded_glob,
    env_path,
    env_path_value,
    expanded,
    is_remote,
    listed_files,
    load_json_document,
    load_json_secure,
    load_toml_document,
    matches_any,
    safe_basename,
    walked_files,
)

Claim = dict[str, Any]
Context = dict[str, Any]

MANAGED_CLAUDE_MD = (
    Path("/etc/claude-code/CLAUDE.md"),
    Path("/Library/Application Support/ClaudeCode/CLAUDE.md"),
)

def user_lane(context: Context) -> bool:
    return context.get("scope", "all") != "project"

def claim(path: Path, agent: str, kind: str, scope: str, order: int, note: str = "") -> Claim:
    return {
        "path": path,
        "agent": agent,
        "kind": kind,
        "scope": scope,
        "loadOrder": order,
        "discovery": "documented",
        "note": note,
        "excluded": False,
        "inline": None,
    }

def claude_settings_documents(context: Context) -> list[tuple[Path, dict[str, Any], str]]:
    home = context["home"]
    root = context["projectRoot"]
    candidates = [
        (Path("/etc/claude-code/managed-settings.json"), "managed"),
        (home / ".claude" / "settings.json", "user"),
    ]
    if root:
        base = expanded(root)
        candidates.append((base / ".claude" / "settings.json", "project"))
        candidates.append((base / ".claude" / "settings.local.json", "local"))
    found = []
    for path, scope in candidates:
        document = load_json_document(path)
        if document:
            found.append((path, document, scope))
    return found

def claude_settings(context: Context) -> dict[str, Any]:
    excludes: list[str] = []
    directory = ""
    inline: list[tuple[Path, int, str]] = []
    for path, document, scope in claude_settings_documents(context):
        patterns = document.get("claudeMdExcludes")
        if isinstance(patterns, list):
            excludes.extend(str(entry) for entry in patterns[:MAX_INSTRUCTION_ENTRIES] if isinstance(entry, str))
        value = document.get("autoMemoryDirectory")
        if isinstance(value, str) and (value.startswith("/") or value.startswith("~/")):
            directory = value
        managed = document.get("claudeMd")
        if isinstance(managed, str) and managed.strip() and scope == "managed":
            inline.append((path, len(managed.encode("utf-8", "replace")), scope))
    return {"excludes": excludes, "autoMemoryDirectory": directory, "inline": inline}

def named_in(directory: Path, names: tuple[str, ...]) -> list[Path]:
    watch_path(directory, directory=True)
    found = []
    for name in names:
        candidate = directory / name
        try:
            if candidate.is_file():
                found.append(candidate)
        except OSError:
            continue
    return found

def chain_from(root: str, cwd: Path, names: tuple[str, ...]) -> list[tuple[Path, int]]:
    if not root:
        return []
    stop = expanded(root)
    chain = list(reversed(ancestors_of(cwd, stop)))
    results: list[tuple[Path, int]] = []
    for order, directory in enumerate(chain):
        for candidate in named_in(directory, names):
            results.append((candidate, order))
    return results

def claude_code(context: Context, budget: Budget) -> list[Claim]:
    home = context["home"]
    root = context["projectRoot"]
    cwd = context["cwd"]
    settings = claude_settings(context)
    context["claudeSettings"] = settings
    claims: list[Claim] = []
    if user_lane(context):
        for path, size, scope in settings["inline"]:
            entry = claim(path, "claude-code", "instructions", scope, 1, "inline claudeMd")
            entry["inline"] = size
            claims.append(entry)
        for managed in MANAGED_CLAUDE_MD:
            watch_path(managed)
            try:
                if managed.is_file():
                    claims.append(claim(managed, "claude-code", "instructions", "managed", 0))
            except OSError:
                continue
        for candidate in named_in(home / ".claude", ("CLAUDE.md",)):
            claims.append(claim(candidate, "claude-code", "instructions", "user", 10))
        for candidate in listed_files(home / ".claude" / "rules"):
            claims.append(claim(candidate, "claude-code", "rules", "user", 11))
    for candidate, order in chain_from(root, cwd, ("CLAUDE.md", "CLAUDE.local.md")):
        scope = "local" if candidate.name == "CLAUDE.local.md" else "project"
        claims.append(claim(candidate, "claude-code", "instructions", scope, 20 + order))
    if root:
        base = expanded(root)
        for candidate in named_in(base / ".claude", ("CLAUDE.md",)):
            claims.append(claim(candidate, "claude-code", "instructions", "project", 20))
        for candidate in walked_files(base / ".claude" / "rules"):
            claims.append(claim(candidate, "claude-code", "rules", "project", 21))
    if user_lane(context):
        claims.extend(claude_auto_memory(context, budget))
    excludes = settings["excludes"]
    if excludes:
        for entry in claims:
            if entry["inline"] is None and matches_any(entry["path"], excludes):
                entry["excluded"] = True
    return claims

def auto_memory_roots(context: Context) -> list[Path]:
    settings = context.get("claudeSettings") or {}
    configured = str(settings.get("autoMemoryDirectory") or "")
    if configured:
        return [expanded(configured)]
    projects = context["home"] / ".claude" / "projects"
    return [entry / "memory" for entry in bounded_directories(projects, MAX_SOURCES, follow_symlinks=True)]

def claude_auto_memory(context: Context, budget: Budget) -> list[Claim]:
    claims: list[Claim] = []
    directories = auto_memory_roots(context)
    for memory in directories:
        if not budget.take_source():
            break
        label = memory.parent.name if memory.name == "memory" else memory.name
        index = memory / "MEMORY.md"
        watch_path(index)
        try:
            if not index.is_file():
                continue
        except OSError:
            continue
        claims.append(claim(index, "claude-code", "auto-memory", "user", 30, label))
        for candidate in listed_files(memory):
            if candidate.name != "MEMORY.md":
                claims.append(claim(candidate, "claude-code", "auto-memory", "user", 31, label))
    return claims

def codex_fallback_names(codex_home: Path) -> list[str]:
    document = load_toml_document(codex_home / "config.toml")
    values = document.get("project_doc_fallback_filenames")
    if not isinstance(values, list):
        return []
    names = []
    for value in values[:MAX_FALLBACK_NAMES]:
        name = safe_basename(value)
        if name and name not in names:
            names.append(name)
    return names

def codex(context: Context, budget: Budget) -> list[Claim]:
    home = context["home"]
    codex_home = env_path("CODEX_HOME", context["environ"]) or (home / ".codex")
    claims: list[Claim] = []
    for name, order in (("AGENTS.override.md", 0), ("AGENTS.md", 1)) if user_lane(context) else ():
        for candidate in named_in(codex_home, (name,)):
            claims.append(claim(candidate, "codex", "instructions", "user", 10 + order))
            break
    documented = ("AGENTS.override.md", "AGENTS.md")
    for candidate, order in chain_from(context["projectRoot"], context["cwd"], documented):
        claims.append(claim(candidate, "codex", "instructions", "project", 20 + order))
    fallbacks = tuple(name for name in codex_fallback_names(codex_home) if name not in documented)
    if fallbacks:
        for candidate, order in chain_from(context["projectRoot"], context["cwd"], fallbacks):
            claims.append(claim(candidate, "codex", "instructions", "project", 40 + order, "config fallback name"))
    return claims

def opencode(context: Context, budget: Budget) -> list[Claim]:
    home = context["home"]
    config_home = env_path("XDG_CONFIG_HOME", context["environ"]) or (home / ".config")
    claims: list[Claim] = []
    if user_lane(context):
        for candidate in named_in(config_home / "opencode", ("AGENTS.md",)):
            claims.append(claim(candidate, "opencode", "instructions", "user", 10))
        for candidate in named_in(home / ".claude", ("CLAUDE.md",)):
            claims.append(claim(candidate, "opencode", "instructions", "user", 11, "claude-code fallback"))
    for candidate, order in chain_from(context["projectRoot"], context["cwd"], ("AGENTS.md", "CLAUDE.md")):
        claims.append(claim(candidate, "opencode", "instructions", "project", 20 + order))
    claims.extend(opencode_instructions(context, config_home, budget))
    return claims

def opencode_instructions(context: Context, config_home: Path, budget: Budget) -> list[Claim]:
    sources = [(config_home / "opencode" / "opencode.json", "user")] if user_lane(context) else []
    root = context["projectRoot"]
    if root:
        sources.append((expanded(root) / "opencode.json", "project"))
    claims: list[Claim] = []
    for path, scope in sources:
        document = load_json_document(path)
        entries = document.get("instructions")
        if not isinstance(entries, list) or not budget.take_source():
            continue
        remote = 0
        for entry in entries[:MAX_INSTRUCTION_ENTRIES]:
            if not isinstance(entry, str):
                continue
            if is_remote(entry):
                remote += 1
                continue
            for candidate in bounded_glob(path.parent, entry):
                claims.append(claim(candidate, "opencode", "instructions", scope, 30, "instructions entry"))
        if remote:
            marker = claim(path, "opencode", "instructions", scope, 31,
                           str(remote) + " remote entries not fetched")
            marker["inline"] = 0
            claims.append(marker)
    return claims

def pi(context: Context, budget: Budget) -> list[Claim]:
    home = context["home"]
    agent_home = home / ".pi" / "agent"
    claims: list[Claim] = []
    for name, kind in (("AGENTS.md", "instructions"), ("CLAUDE.md", "instructions"),
                       ("SYSTEM.md", "system-prompt"), ("APPEND_SYSTEM.md", "system-prompt")) if user_lane(context) else ():
        for candidate in named_in(agent_home, (name,)):
            claims.append(claim(candidate, "pi", kind, "user", 10))
    for candidate, order in chain_from(context["projectRoot"], context["cwd"],
                                       ("AGENTS.override.md", "AGENTS.md", "CLAUDE.md")):
        claims.append(claim(candidate, "pi", "instructions", "project", 20 + order))
    if context["projectRoot"]:
        base = expanded(context["projectRoot"]) / ".pi"
        for name in ("SYSTEM.md", "APPEND_SYSTEM.md"):
            for candidate in named_in(base, (name,)):
                claims.append(claim(candidate, "pi", "system-prompt", "project", 25))
    return claims

def copilot_extra_dirs(context: Context) -> list[Path]:
    raw = env_path_value("COPILOT_CUSTOM_INSTRUCTIONS_DIRS", context["environ"])
    found = []
    for piece in raw.split(",")[:MAX_ENV_DIRS]:
        if piece == "":
            continue
        candidate = expanded(piece)
        watch_path(candidate, directory=True)
        try:
            if candidate.is_dir():
                found.append(candidate)
        except OSError:
            continue
    return found

def copilot_cli(context: Context, budget: Budget) -> list[Claim]:
    home = context["home"]
    copilot_home = env_path("COPILOT_HOME", context["environ"]) or (home / ".copilot")
    claims: list[Claim] = []
    if user_lane(context):
        for candidate in named_in(copilot_home, ("copilot-instructions.md",)):
            claims.append(claim(candidate, "copilot-cli", "instructions", "user", 10))
        for candidate in walked_files(copilot_home / "instructions", (".md",)):
            if candidate.name.endswith(".instructions.md"):
                claims.append(claim(candidate, "copilot-cli", "instructions", "user", 11))
        for directory in copilot_extra_dirs(context):
            if not budget.take_source():
                break
            for candidate in named_in(directory, ("AGENTS.md",)):
                claims.append(claim(candidate, "copilot-cli", "instructions", "user", 12, "custom instructions dir"))
            for candidate in walked_files(directory / ".github" / "instructions", (".md",)):
                if candidate.name.endswith(".instructions.md"):
                    claims.append(claim(candidate, "copilot-cli", "instructions", "user", 12, "custom instructions dir"))
    if context["projectRoot"]:
        base = expanded(context["projectRoot"])
        for candidate in named_in(base / ".github", ("copilot-instructions.md",)):
            claims.append(claim(candidate, "copilot-cli", "instructions", "project", 20))
        for candidate in walked_files(base / ".github" / "instructions", (".md",)):
            if candidate.name.endswith(".instructions.md"):
                claims.append(claim(candidate, "copilot-cli", "instructions", "project", 21))
    for candidate, order in chain_from(context["projectRoot"], context["cwd"],
                                       ("AGENTS.md", "CLAUDE.md", "GEMINI.md")):
        claims.append(claim(candidate, "copilot-cli", "instructions", "project", 22 + order))
    if context["projectRoot"]:
        for candidate in named_in(expanded(context["projectRoot"]) / ".claude", ("CLAUDE.md",)):
            claims.append(claim(candidate, "copilot-cli", "instructions", "project", 23))
    return claims

def antigravity(context: Context, budget: Budget) -> list[Claim]:
    home = context["home"]
    claims: list[Claim] = []
    if user_lane(context):
        for candidate in named_in(home / ".gemini", ("GEMINI.md",)):
            claims.append(claim(candidate, "antigravity", "instructions", "user", 10))
    if context["projectRoot"]:
        base = expanded(context["projectRoot"])
        for folder in (".agents/rules", ".agent/rules"):
            for candidate in walked_files(base / folder):
                claims.append(claim(candidate, "antigravity", "rules", "project", 20))
    plugins = home / ".gemini" / "antigravity-cli" / "plugins"
    directories = bounded_directories(plugins, MAX_SOURCES, follow_symlinks=True) if user_lane(context) else []
    for directory in directories:
        if not budget.take_source():
            break
        for candidate in walked_files(directory / "rules"):
            claims.append(claim(candidate, "antigravity", "rules", "plugin", 30, directory.name))
    return claims

ADAPTERS: tuple[tuple[str, Callable[[Context, Budget], list[Claim]]], ...] = (
    ("claude-code", claude_code),
    ("codex", codex),
    ("opencode", opencode),
    ("pi", pi),
    ("copilot-cli", copilot_cli),
    ("antigravity", antigravity),
)
