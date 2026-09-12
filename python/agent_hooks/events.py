from __future__ import annotations

from .adapters import COPILOT_PASCAL_ALIASES

CANONICAL_EVENTS: tuple[tuple[str, dict[str, str]], ...] = (
    ("SessionStart", {"claude-code": "SessionStart", "codex": "SessionStart", "copilot-cli": "sessionStart"}),
    ("SessionEnd", {"claude-code": "SessionEnd", "codex": "SessionEnd", "copilot-cli": "sessionEnd"}),
    ("UserPromptSubmit", {"claude-code": "UserPromptSubmit", "codex": "UserPromptSubmit", "copilot-cli": "userPromptSubmitted"}),
    ("PreToolUse", {"claude-code": "PreToolUse", "codex": "PreToolUse",
                    "copilot-cli": "preToolUse", "antigravity": "PreToolUse"}),
    ("PostToolUse", {"claude-code": "PostToolUse", "codex": "PostToolUse",
                     "copilot-cli": "postToolUse", "antigravity": "PostToolUse"}),
    ("PostToolUseFailure", {"claude-code": "PostToolUseFailure", "copilot-cli": "postToolUseFailure"}),
    ("PermissionRequest", {"claude-code": "PermissionRequest", "codex": "PermissionRequest",
                           "copilot-cli": "permissionRequest"}),
    ("Notification", {"claude-code": "Notification",
                      "copilot-cli": "notification"}),
    ("Stop", {"claude-code": "Stop", "codex": "Stop",
              "copilot-cli": "agentStop", "antigravity": "Stop"}),
    ("SubagentStart", {"claude-code": "SubagentStart", "codex": "SubagentStart",
                       "copilot-cli": "subagentStart"}),
    ("SubagentStop", {"claude-code": "SubagentStop", "codex": "SubagentStop",
                      "copilot-cli": "subagentStop"}),
    ("PreCompact", {"claude-code": "PreCompact", "codex": "PreCompact",
                    "copilot-cli": "preCompact"}),
    ("PostCompact", {"claude-code": "PostCompact", "codex": "PostCompact"}),
)

TO_AGENT: dict[str, dict[str, str]] = {name: dict(table) for name, table in CANONICAL_EVENTS}
TO_CANONICAL: dict[str, dict[str, str]] = {}
for _name, _table in CANONICAL_EVENTS:
    for _agent, _event in _table.items():
        TO_CANONICAL.setdefault(_agent, {})[_event] = _name

def native_event(agent: str, event: str) -> str:
    if agent == "copilot-cli":
        return COPILOT_PASCAL_ALIASES.get(event, event)
    return event

def canonical(agent: str, event: str) -> str:
    name = native_event(agent, event)
    return TO_CANONICAL.get(agent, {}).get(name) or f"{agent}:{name}"

def mapped_event(source_agent: str, event: str, target_agent: str) -> str | None:
    key = canonical(source_agent, event)
    if key in TO_AGENT:
        return TO_AGENT[key].get(target_agent)
    return native_event(source_agent, event) if target_agent == source_agent else None
