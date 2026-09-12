from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Callable

from fileblade_paths import NativePath, display

from .redaction import enabled_state, payload_material, safe_summary
from .records import identity
from .safeio import (
    MAX_ITEMS_PER_SOURCE,
    artifact_metrics,
    Budget,
    existing_file,
    expanded,
    is_plain_dir,
    load_json,
    load_toml,
    scan_dir,
    stable_id,
)

Context = dict[str, Any]

CLAUDE_EVENTS = (
    "SessionStart", "Setup", "UserPromptSubmit", "UserPromptExpansion", "PreToolUse",
    "PermissionRequest", "PermissionDenied", "PostToolUse", "PostToolUseFailure",
    "PostToolBatch", "Notification", "MessageDisplay", "SubagentStart", "SubagentStop",
    "TaskCreated", "TaskCompleted", "Stop", "StopFailure", "TeammateIdle",
    "InstructionsLoaded", "ConfigChange", "CwdChanged", "DirectoryAdded", "FileChanged",
    "WorktreeCreate", "WorktreeRemove", "PreCompact", "PostCompact", "PreModelSwitch",
    "PostModelSwitch", "Elicitation", "ElicitationResult", "SessionEnd",
)
CODEX_EVENTS = (
    "SessionStart", "SessionEnd", "PreToolUse", "PermissionRequest", "PostToolUse",
    "PreCompact", "PostCompact", "UserPromptSubmit", "SubagentStart", "SubagentStop", "Stop", "Interrupt",
)
COPILOT_EVENTS = (
    "sessionStart", "sessionEnd", "userPromptSubmitted", "userPromptTransformed", "preToolUse",
    "postToolUse", "postToolUseFailure", "agentStop", "subagentStart", "subagentStop",
    "errorOccurred", "preCompact", "permissionRequest", "notification",
)
COPILOT_PASCAL_ALIASES = {
    "SessionStart": "sessionStart",
    "SessionEnd": "sessionEnd",
    "UserPromptSubmit": "userPromptSubmitted",
    "PreToolUse": "preToolUse",
    "PostToolUse": "postToolUse",
    "PostToolUseFailure": "postToolUseFailure",
    "Stop": "agentStop",
    "SubagentStop": "subagentStop",
    "ErrorOccurred": "errorOccurred",
    "PreCompact": "preCompact",
    "PermissionRequest": "permissionRequest",
    "Notification": "notification",
}
COPILOT_KNOWN = COPILOT_EVENTS + tuple(COPILOT_PASCAL_ALIASES)
COPILOT_MANIFESTS = (
    (".plugin", "plugin.json"), ("plugin.json",),
    (".github", "plugin", "plugin.json"), (".claude-plugin", "plugin.json"),
)
ANTIGRAVITY_EVENTS = ("PreToolUse", "PostToolUse", "PreInvocation", "PostInvocation", "Stop")

def row(agent: str, path: Path, scope: str, event: str, index: int, entry: dict[str, Any], record_id: str,
        support: str = "documented", enabled: bool | None = None, note: str = "",
        precedence: int | None = None) -> dict[str, Any]:
    source = {"kind": path.suffix.lstrip(".") or "file", "path": NativePath(str(path)), "name": display(path.name)}
    try:
        target = path.resolve(strict=True)
        if target != path.absolute():
            source["realpath"] = NativePath(str(target))
    except (OSError, RuntimeError):
        pass
    return {
        "precedence": precedence,
        "id": record_id,
        "agent": agent,
        "source": source,
        "scope": scope,
        "supportStatus": support,
        "event": event,
        "index": index,
        "enabled": enabled,
        "summary": safe_summary(entry),
        "metrics": artifact_metrics(path, payload_material(entry)),
        "note": note[:80],
        "badges": [],
    }

def location_row(agent: str, path: Path, scope: str, note: str, count: int) -> dict[str, Any]:
    source = {"kind": "location", "path": NativePath(str(path)), "name": display(path.name)}
    try:
        target = path.resolve(strict=True)
        if target != path.absolute():
            source["realpath"] = NativePath(str(target))
    except (OSError, RuntimeError):
        pass
    return {
        "id": stable_id(agent, str(path), "location"),
        "agent": agent,
        "source": source,
        "scope": scope,
        "supportStatus": "code-hosted",
        "event": "",
        "index": 0,
        "enabled": None,
        "summary": {"label": "", "labelSource": "", "type": "code", "fields": [], "digest": "", "payloadBytes": 0,
                    "envCount": 0, "matcher": "none", "timeoutSeconds": None, "hasCondition": False},
        "metrics": artifact_metrics(path, ""),
        "note": note[:80],
        "entries": count,
        "badges": ["not-inspected"],
    }

def event_map_rows(agent: str, path: Path, scope: str, mapping: dict[str, Any], known: tuple[str, ...],
                   budget: Budget, container: str = "hooks", enabled_default: Any = None,
                   namespace: str = "") -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for event, definitions in mapping.items():
        if not isinstance(event, str) or not isinstance(definitions, list):
            continue
        support = "documented" if event in known else "undocumented-event"
        for group_index, group in enumerate(definitions[:MAX_ITEMS_PER_SOURCE]):
            if not isinstance(group, dict):
                continue
            entries = group.get(container)
            candidates = entries if isinstance(entries, list) else [group]
            for entry_index, entry in enumerate(candidates[:MAX_ITEMS_PER_SOURCE]):
                if not isinstance(entry, dict) or not budget.take_row():
                    break
                merged = dict(group)
                merged.pop(container, None)
                merged.update(entry)
                index = group_index * MAX_ITEMS_PER_SOURCE + entry_index
                record_id = identity(agent, str(path), event, index, group, entry, namespace)
                rows.append(row(agent, path, scope, event, index, merged, record_id,
                                support, enabled_state(merged, enabled_default)))
    return rows

def documents(agent: str, sources: list[tuple[Path, str]], budget: Budget,
              loader: Callable[[Path], dict[str, Any] | None]) -> list[tuple[Path, str, dict[str, Any]]]:
    found: list[tuple[Path, str, dict[str, Any]]] = []
    for path, scope in sources:
        if existing_file(path) is None:
            continue
        budget.note_source(agent, path.absolute())
        if not budget.take_source():
            continue
        document = loader(path)
        if isinstance(document, dict):
            found.append((path, scope, document))
    return found

def hook_mapping(document: dict[str, Any]) -> dict[str, Any] | None:
    mapping = document.get("hooks")
    return mapping if isinstance(mapping, dict) else None

def root_event_mapping(document: dict[str, Any]) -> dict[str, Any] | None:
    mapping = hook_mapping(document)
    if mapping is not None:
        return mapping
    events = {key: value for key, value in document.items() if isinstance(key, str) and isinstance(value, list)}
    return events if events else None

def settings_rows(agent: str, sources: list[tuple[Path, str]], known: tuple[str, ...], budget: Budget,
                  allow_comments: bool = False, mapping_of: Callable[[dict[str, Any]], dict[str, Any] | None] = hook_mapping) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path, scope, document in documents(agent, sources, budget, lambda p: load_json(p, allow_comments)):
        mapping = mapping_of(document)
        if mapping is not None:
            rows.extend(event_map_rows(agent, path, scope, mapping, known, budget))
    return rows

def project_path(context: Context, *parts: str) -> Path | None:
    root = context["projectRoot"]
    return expanded(root).joinpath(*parts) if root else None

PROJECT_SCOPES = frozenset({"project", "local"})

def user_lane(context: Context) -> bool:
    return context.get("scope", "all") != "project"

def lane_sources(context: Context, sources: list) -> list:
    scope = context.get("scope", "all")
    if scope == "project":
        return [entry for entry in sources if entry[1] in PROJECT_SCOPES]
    if scope == "user":
        return [entry for entry in sources if entry[1] not in PROJECT_SCOPES]
    return sources

def scoped(context: Context, home_path: Path, project_parts: tuple[str, ...],
           lane: bool = True) -> list[tuple[Path, str]]:
    sources = [(home_path, "user")]
    candidate = project_path(context, *project_parts)
    if candidate is not None:
        sources.append((candidate, "project"))
    return lane_sources(context, sources) if lane else sources

def claude_code(context: Context, budget: Budget) -> list[dict[str, Any]]:
    home = context["home"]
    sources: list[tuple[Path, str]] = [
        (Path("/etc/claude-code/managed-settings.json"), "managed"),
        (home / ".claude" / "settings.json", "user"),
    ]
    for name, scope in (("settings.json", "project"), ("settings.local.json", "local")):
        candidate = project_path(context, ".claude", name)
        if candidate is not None:
            sources.append((candidate, scope))
    rows = settings_rows("claude-code", lane_sources(context, sources), CLAUDE_EVENTS, budget)
    if not user_lane(context):
        return rows
    plugin_files = [
        (path, "plugin")
        for path in scan_dir(home / ".claude" / "plugins" / "cache", ".json", 5)
        if path.name == "hooks.json" and path.parent.name == "hooks"
    ]
    rows.extend(settings_rows("claude-code", plugin_files, CLAUDE_EVENTS, budget))
    return rows

def codex(context: Context, budget: Budget) -> list[dict[str, Any]]:
    home = context["home"]
    override = context["environ"].get("CODEX_HOME", "")
    codex_home = expanded(override) if override else home / ".codex"
    rows = settings_rows("codex", scoped(context, codex_home / "hooks.json", (".codex", "hooks.json")),
                         CODEX_EVENTS, budget, mapping_of=root_event_mapping)
    toml_sources = scoped(context, codex_home / "config.toml", (".codex", "config.toml"), lane=False)
    for path, scope, document in documents("codex", lane_sources(context, toml_sources), budget, load_toml):
        mapping = hook_mapping(document)
        if mapping is not None:
            rows.extend(event_map_rows("codex", path, scope, mapping, CODEX_EVENTS, budget))
    if rows and not codex_enabled(toml_sources):
        mark(rows, "feature-off", "codex hooks feature is not enabled")
    if not user_lane(context):
        return rows
    return rows + codex_profile_rows(agent_home=codex_home, budget=budget)

def codex_profile_paths(agent_home: Path) -> list[Path]:
    return [path for path in scan_dir(agent_home, ".toml")
            if path.name.endswith(".config.toml") and path.name != "config.toml"]

def codex_profile_rows(*, agent_home: Path, budget: Budget) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in codex_profile_paths(agent_home):
        budget.note_source("codex", path.absolute())
        if not budget.take_source():
            break
        document = load_toml(path)
        mapping = hook_mapping(document) if isinstance(document, dict) else None
        if mapping is None:
            continue
        entries = event_map_rows("codex", path, "profile", mapping, CODEX_EVENTS, budget)
        profile = path.name[: -len(".config.toml")]
        for entry in entries:
            entry["enabled"] = None
            entry["profile"] = profile
        rows.extend(mark(entries, "profile-unselected",
                         "applies only when codex runs with --profile"))
    return rows

def codex_enabled(sources: list[tuple[Path, str]]) -> bool:
    for path, _scope in sources:
        if existing_file(path) is None:
            continue
        document = load_toml(path)
        features = document.get("features") if isinstance(document, dict) else None
        if not isinstance(features, dict):
            continue
        for key in ("hooks", "codex_hooks"):
            if features.get(key) is True:
                return True
    return False

def copilot_home(context: Context) -> Path:
    override = context["environ"].get("COPILOT_HOME", "")
    return expanded(override) if override else context["home"] / ".copilot"

def mark(rows: list[dict[str, Any]], badge: str, note: str = "") -> list[dict[str, Any]]:
    for entry in rows:
        if badge not in entry["badges"]:
            entry["badges"].append(badge)
        if note and not entry["note"]:
            entry["note"] = note[:80]
    return rows

def alias_badges(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    for entry in rows:
        if entry["event"] in COPILOT_PASCAL_ALIASES:
            entry["badges"].append("vscode-alias")
            entry["canonicalEvent"] = COPILOT_PASCAL_ALIASES[entry["event"]]
    return rows

def copilot_policy_rows(context: Context, budget: Budget) -> list[dict[str, Any]]:
    directory = context["etcRoot"] / "github-copilot" / "policy.d"
    owner = context.get("policyOwnerUid", 0)
    rows: list[dict[str, Any]] = []
    for path in scan_dir(directory, ".json"):
        budget.note_source("copilot-cli", path.absolute())
        if not budget.take_source():
            break
        document = load_json(path, owner_uid=owner)
        if not isinstance(document, dict) or document.get("version") != 1:
            continue
        mapping = hook_mapping(document)
        if mapping is None:
            continue
        entries = event_map_rows("copilot-cli", path, "policy", mapping, COPILOT_KNOWN, budget)
        rows.extend(mark(entries, "policy", "policy hooks ignore disableAllHooks"))
    return alias_badges(rows)

def copilot_file_rows(context: Context, budget: Budget) -> list[dict[str, Any]]:
    files: list[tuple[Path, str]] = []
    if user_lane(context):
        files.extend((path, "user") for path in scan_dir(copilot_home(context) / "hooks", ".json"))
    project = project_path(context, ".github", "hooks")
    if project is not None:
        files.extend((path, "project") for path in scan_dir(project, ".json"))
    rows: list[dict[str, Any]] = []
    for path, scope, document in documents("copilot-cli", files, budget, load_json):
        if document.get("version") != 1:
            continue
        mapping = hook_mapping(document)
        if mapping is None:
            continue
        entries = event_map_rows("copilot-cli", path, scope, mapping, COPILOT_KNOWN, budget)
        if document.get("disableAllHooks") is True:
            for entry in entries:
                entry["enabled"] = False
            mark(entries, "file-disabled", "disableAllHooks in this hook file")
        rows.extend(entries)
    return alias_badges(rows)

def copilot_settings_sources(context: Context) -> list[tuple[Path, str, bool]]:
    sources: list[tuple[Path, str, bool]] = [(copilot_home(context) / "settings.json", "user", False)]
    for parts in ((".github", "copilot", "settings.json"), (".github", "copilot", "settings.local.json"),
                  (".claude", "settings.json"), (".claude", "settings.local.json")):
        candidate = project_path(context, *parts)
        if candidate is not None:
            sources.append((candidate, "project", True))
    return sources

def copilot_settings_rows(context: Context, budget: Budget) -> tuple[list[dict[str, Any]], bool]:
    rows: list[dict[str, Any]] = []
    repository_disabled = False
    for path, scope, repository in lane_sources(context, copilot_settings_sources(context)):
        if existing_file(path) is None:
            continue
        budget.note_source("copilot-cli", path.absolute())
        if not budget.take_source():
            continue
        document = load_json(path)
        if not isinstance(document, dict):
            continue
        disabled = document.get("disableAllHooks") is True
        if disabled and repository:
            repository_disabled = True
        mapping = hook_mapping(document)
        if mapping is None:
            continue
        entries = event_map_rows("copilot-cli", path, scope, mapping, COPILOT_KNOWN, budget)
        mark(entries, "settings-inline")
        if disabled and not repository:
            for entry in entries:
                entry["enabled"] = False
            mark(entries, "file-disabled", "disableAllHooks in user settings")
        rows.extend(entries)
    return alias_badges(rows), repository_disabled

def copilot_plugin_directories(context: Context) -> list[Path]:
    installed = copilot_home(context) / "installed-plugins"
    found: list[Path] = []
    for marketplace in scan_dir_directories(installed):
        found.extend(scan_dir_directories(marketplace))
    return found[:MAX_ITEMS_PER_SOURCE]

def scan_dir_directories(root: Path) -> list[Path]:
    if not is_plain_dir(root):
        return []
    result: list[Path] = []
    try:
        with os.scandir(root) as entries:
            for count, entry in enumerate(entries):
                if count >= MAX_ITEMS_PER_SOURCE:
                    break
                if entry.is_dir(follow_symlinks=False):
                    result.append(Path(entry.path))
    except OSError:
        return sorted(result)
    return sorted(result)

def copilot_plugin_name(plugin: Path, budget: Budget) -> str:
    for parts in COPILOT_MANIFESTS:
        manifest_path = plugin.joinpath(*parts)
        if existing_file(manifest_path) is None or not budget.take_source():
            continue
        manifest = load_json(manifest_path)
        if isinstance(manifest, dict) and isinstance(manifest.get("name"), str):
            return manifest["name"]
    return plugin.name

def copilot_plugin_rows(context: Context, budget: Budget) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for plugin in copilot_plugin_directories(context):
        name = copilot_plugin_name(plugin, budget)
        for parts in (("hooks.json",), ("hooks", "hooks.json")):
            path = plugin.joinpath(*parts)
            if existing_file(path) is None:
                continue
            budget.note_source("copilot-cli", path.absolute())
            if not budget.take_source():
                continue
            document = load_json(path)
            mapping = hook_mapping(document) if isinstance(document, dict) else None
            if mapping is None:
                continue
            entries = event_map_rows("copilot-cli", path, "plugin", mapping, COPILOT_KNOWN, budget)
            for entry in entries:
                entry["enabled"] = None
                entry["plugin"] = name
            rows.extend(mark(entries, "enable-state-undocumented",
                             "plugin enable state has no documented on-disk location"))
            break
    return alias_badges(rows)

def copilot_cli(context: Context, budget: Budget) -> list[dict[str, Any]]:
    policy = copilot_policy_rows(context, budget) if user_lane(context) else []
    files = copilot_file_rows(context, budget)
    settings, repository_disabled = copilot_settings_rows(context, budget)
    plugins = copilot_plugin_rows(context, budget) if user_lane(context) else []
    others = files + settings + plugins
    if repository_disabled:
        for entry in others:
            entry["enabled"] = False
        mark(others, "repository-disabled", "disableAllHooks in repository settings")
    return policy + others

def antigravity(context: Context, budget: Budget) -> list[dict[str, Any]]:
    sources = scoped(context, context["home"] / ".gemini" / "config" / "hooks.json",
                     (".agents", "hooks.json"))
    rows: list[dict[str, Any]] = []
    for path, scope, document in documents("antigravity", sources, budget, load_json):
        for name, definition in list(document.items())[:MAX_ITEMS_PER_SOURCE]:
            if not isinstance(name, str) or not isinstance(definition, dict):
                continue
            enabled = definition.get("enabled")
            mapping = {event: definition[event] for event in ANTIGRAVITY_EVENTS
                       if isinstance(definition.get(event), list)}
            entries = event_map_rows("antigravity", path, scope, mapping, ANTIGRAVITY_EVENTS, budget,
                                     enabled_default=enabled if isinstance(enabled, bool) else None, namespace=name)
            for entry in entries:
                entry["group"] = name
                entry["note"] = name[:80]
            rows.extend(entries)
    return rows

def code_hosted(agent: str, context: Context, budget: Budget, directories: list[tuple[Path, str]],
                suffixes: tuple[str, ...], depth: int, note: str) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for directory, scope in directories:
        files: list[Path] = []
        for suffix in suffixes:
            files.extend(scan_dir(directory, suffix, depth))
        if not files or not budget.take_source() or not budget.take_row():
            continue
        rows.append(location_row(agent, directory, scope, note, len(files)))
    return rows

def declared_rows(agent: str, sources: list[tuple[Path, str]], budget: Budget, keys: tuple[str, ...],
                  note: str) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path, scope, document in documents(agent, sources, budget, lambda p: load_json(p, True)):
        declared = 0
        for key in keys:
            value = document.get(key)
            if isinstance(value, list):
                declared += len(value)
        if declared and budget.take_row():
            rows.append(location_row(agent, path, scope, note, declared))
    return rows

def opencode(context: Context, budget: Budget) -> list[dict[str, Any]]:
    override = context["environ"].get("XDG_CONFIG_HOME", "")
    config_home = expanded(override) if override else context["home"] / ".config"
    directories = scoped(context, config_home / "opencode" / "plugins", (".opencode", "plugins"))
    rows = code_hosted("opencode", context, budget, directories, (".js", ".ts"), 1,
                       "plugin directory, hooks defined in code")
    rows.extend(declared_rows("opencode", scoped(context, config_home / "opencode" / "opencode.json",
                                                 ("opencode.json",)), budget,
                              ("plugin",), "declared plugin packages"))
    return rows

def pi(context: Context, budget: Budget) -> list[dict[str, Any]]:
    home = context["home"]
    directories = scoped(context, home / ".pi" / "agent" / "extensions", (".pi", "extensions"))
    rows = code_hosted("pi", context, budget, directories, (".ts", ".js"), 2,
                       "extension directory, hooks defined in code")
    rows.extend(declared_rows("pi", scoped(context, home / ".pi" / "agent" / "settings.json",
                                           (".pi", "settings.json")), budget,
                              ("extensions", "packages"), "declared extensions and packages"))
    return rows

ADAPTERS: tuple[tuple[str, Callable[[Context, Budget], list[dict[str, Any]]]], ...] = (
    ("claude-code", claude_code),
    ("codex", codex),
    ("copilot-cli", copilot_cli),
    ("antigravity", antigravity),
    ("opencode", opencode),
    ("pi", pi),
)
