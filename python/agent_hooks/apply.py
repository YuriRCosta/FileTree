from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from fileblade_paths import NativePath, parse_path, path_text
from fileblade_mutations import Document, Snapshot

from . import discovery, records
from .adapters import copilot_home
from .events import mapped_event
from .redaction import payload_digest, safe_type
from .recovery import RecoveryFull, RecoveryStore
from .safeio import Budget, MAX_FILE_BYTES, bounded_depth, expanded, load_json, load_toml

SCHEMA_VERSION = 1

def recovery_store(home: str = "", environ: dict[str, str] | None = None) -> RecoveryStore:
    variables = dict(environ if environ is not None else os.environ)
    home_path = expanded(home) if home else Path(os.path.expanduser("~"))
    configured = variables.get("XDG_STATE_HOME", "")
    base = expanded(configured) if configured else home_path / ".local" / "state"
    return RecoveryStore(base / "fileblade" / "hooks-recovery")

GROUPED_AGENTS = ("claude-code", "codex")
WRITER_AGENTS = GROUPED_AGENTS + ("copilot-cli", "antigravity")
CODE_HOSTED_AGENTS = ("opencode", "pi")

def result(agent: str, ok: bool, changed: bool, message: str, touched: list[str] | None = None) -> dict[str, Any]:
    return {"agent": agent, "ok": ok, "changed": changed, "message": message,
            "touched": [NativePath(path) for path in touched or []]}

def refusal(agent: str, message: str) -> dict[str, Any]:
    return result(agent, False, False, message)

def target_path(agent: str, context: dict[str, Any]) -> Path:
    home: Path = context["home"]
    if agent == "claude-code":
        return home / ".claude" / "settings.json"
    if agent == "codex":
        override = context["environ"].get("CODEX_HOME", "")
        return (expanded(override) if override else home / ".codex") / "hooks.json"
    if agent == "copilot-cli":
        return copilot_home(context) / "hooks" / "hooks.json"
    return home / ".gemini" / "config" / "hooks.json"

def source_document(row: dict[str, Any]) -> dict[str, Any] | None:
    path = Path(row["source"]["path"])
    if path.suffix == ".toml":
        return load_toml(path)
    return load_json(path, allow_comments=True)

def source_entry(row: dict[str, Any], document: dict[str, Any]) -> dict[str, Any] | None:
    found = records.locate(row, document)
    return merged_entry(found[3], found[4]) if found is not None else None

def merged_entry(group: dict[str, Any], entry: dict[str, Any]) -> dict[str, Any]:
    merged = dict(group)
    merged.pop("hooks", None)
    merged.update(entry)
    return merged

def timeout_seconds(agent: str, entry: dict[str, Any]) -> int | None:
    field = "timeoutSec" if agent == "copilot-cli" else "timeout"
    value = entry.get(field)
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value <= 0:
        return None
    seconds = value
    return max(1, int(round(seconds)))

def portable_hook(agent: str, entry: dict[str, Any]) -> dict[str, Any] | str:
    if safe_type(entry) != "command":
        return "only command hooks can be copied between agents"
    command = entry.get("command") if isinstance(entry.get("command"), str) else entry.get("bash")
    if not isinstance(command, str) or not command.strip():
        return "the source hook has no shell command to copy"
    matcher = entry.get("matcher")
    condition = entry.get("if")
    return {
        "command": command,
        "matcher": matcher if isinstance(matcher, str) and matcher.strip() else None,
        "timeout": timeout_seconds(agent, entry),
        "condition": condition if isinstance(condition, str) and condition.strip() else None,
        "digest": payload_digest(entry),
    }

def hook_entry(agent: str, hook: dict[str, Any]) -> dict[str, Any]:
    if agent == "copilot-cli":
        entry: dict[str, Any] = {"type": "command", "bash": hook["command"]}
        if hook["timeout"] is not None:
            entry["timeoutSec"] = hook["timeout"]
        return entry
    entry = {"type": "command", "command": hook["command"]}
    if hook["timeout"] is not None:
        entry["timeout"] = hook["timeout"]
    if agent == "claude-code" and hook["condition"] is not None:
        entry["if"] = hook["condition"]
    return entry

def group_entry(agent: str, hook: dict[str, Any]) -> dict[str, Any]:
    group: dict[str, Any] = {}
    if hook["matcher"] is not None:
        group["matcher"] = hook["matcher"]
    group["hooks"] = [hook_entry(agent, hook)]
    return group

def entry_matches(group: dict[str, Any], entry: dict[str, Any], digest: str) -> bool:
    return payload_digest(merged_entry(group, entry)) == digest

def contains_digest(definitions: list[Any], digest: str) -> bool:
    for group in definitions:
        if not isinstance(group, dict):
            continue
        entries = group.get("hooks")
        if isinstance(entries, list):
            if any(isinstance(entry, dict) and entry_matches(group, entry, digest) for entry in entries):
                return True
        elif entry_matches({}, group, digest):
            return True
    return False

def without_digest(definitions: list[Any], digest: str) -> tuple[list[Any], bool]:
    kept: list[Any] = []
    changed = False
    for group in definitions:
        if not isinstance(group, dict):
            kept.append(group)
            continue
        entries = group.get("hooks")
        if isinstance(entries, list):
            remaining = [entry for entry in entries
                         if not (isinstance(entry, dict) and entry_matches(group, entry, digest))]
            if len(remaining) != len(entries):
                changed = True
            if remaining:
                trimmed = dict(group)
                trimmed["hooks"] = remaining
                kept.append(trimmed)
            continue
        if entry_matches({}, group, digest):
            changed = True
            continue
        kept.append(group)
    return kept, changed

def load_target(path: Path) -> dict[str, Any] | str:
    try:
        snapshot = Snapshot.read(path, MAX_FILE_BYTES)
    except (OSError, RuntimeError) as error:
        return f"cannot read {path}: {error}"
    raw = snapshot.data
    if raw is None:
        return Document({}, snapshot)
    try:
        parsed = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_fields)
    except (ValueError, RecursionError):
        return f"{path} is not strict JSON (comments or trailing commas are not rewritten)"
    if not isinstance(parsed, dict):
        return f"{path} does not hold a JSON object"
    if not bounded_depth(parsed):
        return f"{path} exceeds the JSON nesting limit"
    return Document(parsed, snapshot)

def unique_fields(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON member")
        result[key] = value
    return result

def write_atomic(path: Path, document: dict[str, Any]) -> None:
    try:
        payload = (json.dumps(document, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode("utf-8")
    except (ValueError, UnicodeError) as error:
        raise OSError("the updated hook file cannot be represented as UTF-8 JSON") from error
    snapshot = document.snapshot if isinstance(document, Document) else Snapshot.read(path, MAX_FILE_BYTES)
    snapshot.write(payload)

def hooks_container(document: dict[str, Any], agent: str) -> dict[str, Any] | str:
    if agent == "copilot-cli":
        version = document.get("version", 1)
        if version != 1:
            return f"copilot hook file version {version!r} is not 1"
        document["version"] = 1
    hooks = document.get("hooks")
    if hooks is None:
        hooks = {}
        document["hooks"] = hooks
    if not isinstance(hooks, dict):
        return "the target's hooks key is not an object"
    return hooks

def event_list(container: dict[str, Any], event: str) -> list[Any] | str:
    definitions = container.get(event)
    if definitions is None:
        definitions = []
        container[event] = definitions
    if not isinstance(definitions, list):
        return f"the target's {event} entry is not an array"
    return definitions

def antigravity_groups(document: dict[str, Any]) -> list[tuple[str, dict[str, Any]]]:
    return [(name, definition) for name, definition in document.items()
            if isinstance(name, str) and isinstance(definition, dict)]

def turn_on(agent: str, event: str, document: dict[str, Any], hook: dict[str, Any]) -> bool | str:
    if agent == "antigravity":
        for _name, definition in antigravity_groups(document):
            definitions = definition.get(event)
            if isinstance(definitions, list) and contains_digest(definitions, hook["digest"]):
                return False
        definition = document.setdefault("hook-" + hook["digest"], {})
        if not isinstance(definition, dict):
            return "the target's hook group is not an object"
        definitions = event_list(definition, event)
        if isinstance(definitions, str):
            return definitions
        definitions.append(group_entry(agent, hook))
        return True
    container = hooks_container(document, agent)
    if isinstance(container, str):
        return container
    definitions = event_list(container, event)
    if isinstance(definitions, str):
        return definitions
    if contains_digest(definitions, hook["digest"]):
        return False
    definitions.append(hook_entry(agent, hook) if agent == "copilot-cli" else group_entry(agent, hook))
    return True

def turn_off(agent: str, event: str, document: dict[str, Any], digest: str) -> bool | str:
    if agent == "antigravity":
        changed = False
        for name, definition in antigravity_groups(document):
            definitions = definition.get(event)
            if not isinstance(definitions, list):
                continue
            kept, removed = without_digest(definitions, digest)
            if not removed:
                continue
            changed = True
            if kept:
                definition[event] = kept
            else:
                definition.pop(event)
            if not any(key != "enabled" for key in definition):
                document.pop(name)
        return changed
    container = hooks_container(document, agent)
    if isinstance(container, str):
        return container
    definitions = container.get(event)
    if not isinstance(definitions, list):
        return False
    kept, changed = without_digest(definitions, digest)
    if changed:
        if kept:
            container[event] = kept
        else:
            container.pop(event)
    return changed

def apply_to_agent(agent: str, row: dict[str, Any], hook: dict[str, Any], state: str,
                   context: dict[str, Any]) -> dict[str, Any]:
    if agent in CODE_HOSTED_AGENTS:
        return refusal(agent, f"{agent} hooks live in code and have no hook file to write")
    if agent not in WRITER_AGENTS:
        return refusal(agent, f"unknown agent {agent!r}")
    if state == "off" and agent == row["agent"]:
        return refusal(agent, "the row's own agent keeps its hook; edit the source file directly")
    event = mapped_event(row["agent"], row["event"], agent)
    if event is None:
        return refusal(agent, f"{agent} has no event equivalent to {row['agent']} {row['event']}")
    path = target_path(agent, context)
    document = load_target(path)
    if isinstance(document, str):
        return refusal(agent, document)
    outcome = turn_on(agent, event, document, hook) if state == "on" else turn_off(agent, event, document, hook["digest"])
    if isinstance(outcome, str):
        return refusal(agent, outcome)
    if not outcome:
        verb = "already present under" if state == "on" else "no matching hook under"
        return result(agent, True, False, f"{verb} {event} in {path}")
    try:
        write_atomic(path, document)
    except OSError as error:
        return refusal(agent, f"could not write {path}: {error.strerror or error}")
    verb = "added under" if state == "on" else "removed from"
    return result(agent, True, True, f"{verb} {event} in {path}", [str(path)])

def expand_agents(agents: list[str], source_agent: str, state: str) -> list[str]:
    expanded_agents: list[str] = []
    for agent in agents:
        if agent == "all":
            expanded_agents.extend(name for name in WRITER_AGENTS if not (state == "off" and name == source_agent))
        else:
            expanded_agents.append(agent)
    unique: list[str] = []
    for agent in expanded_agents:
        if agent not in unique:
            unique.append(agent)
    return unique

def failure(project: str, message: str) -> dict[str, Any]:
    return {"ok": False, "schemaVersion": SCHEMA_VERSION, "project": project, "message": message, "results": []}

def apply(project: str, row_id: str, agents: list[str], state: str, home: str = "",
          environ: dict[str, str] | None = None, etc_root: str = "/etc",
          policy_owner_uid: int = 0, exact: bool = False) -> dict[str, Any]:
    if state not in ("on", "off"):
        return failure("", f"state must be on or off, not {state!r}")
    inventory = discovery.collect(project, home, environ, etc_root, policy_owner_uid, exact)
    root = str(inventory["project"])
    row = next((entry for entry in inventory["items"] if entry["id"] == row_id), None)
    if row is None:
        return failure(root, f"no hook row with id {row_id!r}")
    if row["supportStatus"] == "code-hosted":
        return failure(root, "code-hosted rows describe a directory, not a hook, and cannot be copied")
    document = source_document(row)
    entry = source_entry(row, document) if isinstance(document, dict) else None
    if entry is None:
        return failure(root, f"the source hook is no longer readable at {row['source']['path']}")
    hook = portable_hook(row["agent"], entry)
    if isinstance(hook, str):
        return failure(root, hook)
    if not records.matches(row, document):
        return failure(root, "the source hook changed since it was listed; refresh and retry")
    context = {
        "home": expanded(home) if home else Path(os.path.expanduser("~")),
        "environ": dict(environ if environ is not None else os.environ),
    }
    results = [apply_to_agent(agent, row, hook, state, context)
               for agent in expand_agents(agents, row["agent"], state)]
    failures = [f"{outcome['agent']}: {outcome['message']}" for outcome in results if not outcome["ok"]]
    return {
        "ok": bool(results) and not failures,
        "schemaVersion": SCHEMA_VERSION,
        "project": root,
        "message": "; ".join(failures),
        "results": results,
    }

def source_row(project: str, row_id: str, home: str, environ: dict[str, str] | None, etc_root: str,
               policy_owner_uid: int, exact: bool) -> tuple[str, dict[str, Any] | None]:
    inventory = discovery.collect(project, home, environ, etc_root, policy_owner_uid, exact)
    root = str(inventory["project"])
    return root, next((entry for entry in inventory["items"] if entry["id"] == row_id), None)

def remove(project: str, row_id: str, home: str = "", environ: dict[str, str] | None = None, etc_root: str = "/etc",
           policy_owner_uid: int = 0, exact: bool = False, *, prepare: bool = False,
           expected_payload: dict[str, Any] | None = None, transaction_id: str = "") -> dict[str, Any]:
    root, row = source_row(project, row_id, home, environ, etc_root, policy_owner_uid, exact)
    if row is None:
        return failure(root, f"no hook row with id {row_id!r}")
    if row["supportStatus"] == "code-hosted" or row["source"]["kind"] != "json":
        return failure(root, "only JSON hook definitions can be removed here; edit the source directly")
    path = Path(row["source"]["path"])
    document = load_target(path)
    if isinstance(document, str):
        return failure(root, document)
    if not records.matches(row, document):
        return failure(root, "the source hook changed since it was listed; refresh and retry")
    hook = portable_hook(row["agent"], source_entry(row, document))
    if isinstance(hook, str):
        return failure(root, hook)
    payload = records.detach(row, document)
    payload["source"] = path_text(str(path))
    try:
        payload["target"] = path_text(str(path.resolve(strict=True)))
        encoded = json.dumps(payload, ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode("utf-8")
    except (UnicodeError, ValueError):
        return failure(root, "the hook cannot be preserved as a Unicode undo record; edit the source directly")
    except (OSError, RuntimeError):
        return failure(root, "the source hook path changed; refresh and retry")
    if len(encoded) > 1024 * 1024:
        return failure(root, "the hook exceeds the undo record size limit; edit the source directly")
    context = {
        "project": path_text(str(root)),
        "home": path_text(str(expanded(home) if home else Path(os.path.expanduser("~")))),
        "etcRoot": path_text(str(etc_root)),
        "policyOwnerUid": str(int(policy_owner_uid)),
    }
    if expected_payload is not None and payload != expected_payload:
        return failure(root, "the source hook changed after recovery was prepared; nothing was changed")
    try:
        record_id = recovery_store(home, environ).write(payload, row_id, context, transaction_id)
    except RecoveryFull as error:
        return failure(root, str(error))
    except (OSError, ValueError) as error:
        return failure(root, f"the recovery record could not be stored: {error}")
    if prepare:
        return {"ok": True, "schemaVersion": SCHEMA_VERSION, "project": root, "results": [], "payload": payload,
                "recordId": record_id}
    try:
        write_atomic(path, document)
    except OSError as error:
        return failure(root, f"could not write the source hook: {error.strerror or error}")
    outcome = result(row["agent"], True, True, "removed from " + row["event"], [str(path)])
    return {"ok": outcome["ok"], "schemaVersion": SCHEMA_VERSION, "project": root, "message": "" if outcome["ok"] else outcome["message"],
            "results": [outcome], "payload": payload, "recordId": record_id}

def restore(record_id: str, raw_payload: str | None, home: str = "", environ: dict[str, str] | None = None,
            etc_root: str = "/etc", policy_owner_uid: int = 0) -> dict[str, Any]:
    store = recovery_store(home, environ)
    record = store.read(record_id)
    try:
        payload = record["payload"] if raw_payload is None and record else json.loads(raw_payload)
    except (TypeError, ValueError):
        return failure("", "restore payload is not JSON or its recovery record is unavailable")
    if not isinstance(payload, dict):
        return failure("", "restore payload is not a record")
    store = recovery_store(home, environ)
    record = store.read(record_id)
    if record is not None and record["payload"] != payload:
        record = None
    if record is None:
        record = store.find(payload)
    if record is None:
        return failure("", "no prepared recovery record matches this payload")
    prepared = record["payload"]
    context = record.get("context") or {}
    if prepared.get("format") != 2 or any(not isinstance(prepared.get(key), str) for key in ("agent", "event", "source")):
        return failure("", "restore payload is incomplete")
    agent, event = prepared["agent"], prepared["event"]
    try:
        source = parse_path(prepared["source"])
    except ValueError as error:
        return failure("", str(error))
    if agent not in WRITER_AGENTS or not event or not source.startswith("/"):
        return failure("", "restore payload is incomplete")
    path = Path(source)
    try:
        target = parse_path(str(prepared.get("target", "")))
        if not target.startswith("/") or str(path.resolve(strict=True)) != target:
            return failure("", "the source hook path changed; restore it manually")
        recorded_project = context.get("project") or str(path.parent)
        recorded_home = context.get("home") or home
        recorded_etc = context.get("etcRoot") or etc_root
        try:
            recorded_owner = int(context.get("policyOwnerUid", policy_owner_uid))
        except (TypeError, ValueError):
            recorded_owner = policy_owner_uid
        budget = Budget()
        discovery.collect(recorded_project, recorded_home, environ, recorded_etc, recorded_owner,
                          exact=True, budget=budget)
        if str(path.absolute()) not in budget.hook_sources.get(agent, set()):
            return failure("", "the recorded source is not a known hook configuration file for that agent")
        document = load_target(path)
        if isinstance(document, str):
            return failure("", document)
        changed = records.attach(prepared, document)
        if changed:
            write_atomic(path, document)
    except (ValueError, OSError, RuntimeError) as error:
        return failure("", str(error))
    outcome = result(agent, True, changed, "restored original hook" if changed else "already restored", [source] if changed else [])
    if outcome["ok"]:
        store.mark_restored(str(record["recordId"]))
    return {"ok": outcome["ok"], "schemaVersion": SCHEMA_VERSION, "project": "", "message": "" if outcome["ok"] else outcome["message"],
            "results": [outcome]}
