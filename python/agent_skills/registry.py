from __future__ import annotations

from dataclasses import dataclass

DOCS = {
    "claude-code": "https://code.claude.com/docs/en/skills",
    "claude-code-managed": "https://code.claude.com/docs/en/managed-settings",
    "codex": "https://developers.openai.com/codex/skills",
    "opencode": "https://opencode.ai/docs/skills/",
    "pi": "https://pi.dev/docs/latest/skills",
    "copilot-cli": "https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-skills",
    "antigravity-ide": "https://antigravity.google/docs/ide/skills/",
    "antigravity-cli": "https://antigravity.google/docs/cli/plugins/",
}

AGENT_LABELS = {
    "claude-code": "Claude Code",
    "codex": "Codex",
    "opencode": "OpenCode",
    "pi": "Pi",
    "copilot-cli": "GitHub Copilot CLI",
    "antigravity": "Google Antigravity",
}

MANAGED_CLAUDE = {
    "linux": "/etc/claude-code/.claude/skills",
    "darwin": "/Library/Application Support/ClaudeCode/.claude/skills",
    "win32": "C:\\Program Files\\ClaudeCode\\.claude\\skills",
}

@dataclass(frozen=True)
class Root:
    agent: str
    kind: str
    anchor: str
    path: str
    precedence: int | None
    doc: str

ROOTS: tuple[Root, ...] = (
    Root("claude-code", "managed", "managed-claude", "", 0, DOCS["claude-code-managed"]),
    Root("claude-code", "user", "home", ".claude/skills", 1, DOCS["claude-code"]),
    Root("claude-code", "project", "walk", ".claude/skills", 2, DOCS["claude-code"]),

    Root("codex", "project", "walk", ".agents/skills", 0, DOCS["codex"]),
    Root("codex", "user", "home", ".agents/skills", 3, DOCS["codex"]),
    Root("codex", "system", "absolute", "/etc/codex/skills", 4, DOCS["codex"]),

    Root("opencode", "project", "walk", ".opencode/skills", None, DOCS["opencode"]),
    Root("opencode", "project", "walk", ".claude/skills", None, DOCS["opencode"]),
    Root("opencode", "project", "walk", ".agents/skills", None, DOCS["opencode"]),
    Root("opencode", "user", "home", ".config/opencode/skills", None, DOCS["opencode"]),
    Root("opencode", "user", "home", ".claude/skills", None, DOCS["opencode"]),
    Root("opencode", "user", "home", ".agents/skills", None, DOCS["opencode"]),

    Root("pi", "user", "home", ".pi/agent/skills", 0, DOCS["pi"]),
    Root("pi", "user", "home", ".agents/skills", 1, DOCS["pi"]),
    Root("pi", "project", "walk", ".pi/skills", 2, DOCS["pi"]),
    Root("pi", "project", "walk", ".agents/skills", 3, DOCS["pi"]),

    Root("copilot-cli", "user", "home", ".copilot/skills", None, DOCS["copilot-cli"]),
    Root("copilot-cli", "user", "home", ".agents/skills", None, DOCS["copilot-cli"]),
    Root("copilot-cli", "project", "walk", ".github/skills", None, DOCS["copilot-cli"]),
    Root("copilot-cli", "project", "walk", ".claude/skills", None, DOCS["copilot-cli"]),
    Root("copilot-cli", "project", "walk", ".agents/skills", None, DOCS["copilot-cli"]),


    Root("antigravity", "user", "home", ".gemini/antigravity-cli/skills", None, DOCS["antigravity-cli"]),
    Root("antigravity", "project", "walk", ".agents/skills", None, DOCS["antigravity-ide"]),
)

RESERVED_CLAUDE_SUBDIR = "synced"


PROJECT_MARKERS = (".git",)

def agents() -> tuple[str, ...]:
    return tuple(AGENT_LABELS)

def documented_paths() -> frozenset[str]:
    return frozenset(root.path for root in ROOTS if root.path)
