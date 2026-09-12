from __future__ import annotations

import datetime as dt
import json
import os
import re
import shutil
import sys
import unicodedata
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "python"))

from agent_memory import cli, common, discovery

def write(path: Path, text: str = "# heading\n\nbody\n") -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path

AUDIT_CANARY = "CANARY-CONFIG-CONTENT"

def build_audit_fixtures(home: Path, project: Path) -> None:
    write(home / ".codex" / "config.toml", "\n".join([
        'model = "gpt-5"',
        'project_doc_max_bytes = 65536',
        'project_doc_fallback_filenames = ["CONTRIBUTING.md", "../escape.md", "ok.md", ".hidden"]',
        f'instructions = "{AUDIT_CANARY}"',
        "",
    ]))
    write(project / "CONTRIBUTING.md", "# contributing fallback\n")
    write(project / "ok.md", "# second fallback\n")

    write(home / ".claude" / "settings.json", json.dumps({
        "claudeMdExcludes": [str(project / "CLAUDE.md"), str(project / ".claude" / "rules" / "api.md")],
        "apiKeyHelper": AUDIT_CANARY,
    }))
    write(project / "CLAUDE.md", "@AGENTS.md\n\n# claude project\n")

    write(home / ".config" / "opencode" / "opencode.json", json.dumps({
        "instructions": ["local-guide.md", "docs/*.md", "https://example.invalid/remote.md",
                         "/etc/passwd", "../outside.md"],
        "apiKey": AUDIT_CANARY,
    }))
    write(home / ".config" / "opencode" / "local-guide.md", "# opencode local guide\n")
    write(home / ".config" / "opencode" / "docs" / "one.md", "# globbed one\n")
    write(home / ".config" / "opencode" / "docs" / "two.md", "# globbed two\n")
    write(home / "outside.md", "# must not be reached\n")

    write(home / "copilot-extra" / "AGENTS.md", "# copilot extra dir\n")
    write(home / "copilot-extra" / ".github" / "instructions" / "nested" / "deep.instructions.md", "# copilot nested\n")

def build_home(home: Path) -> None:
    write(home / ".claude" / "CLAUDE.md", "# user claude\n")
    write(home / ".claude" / "rules" / "style.md", "---\npaths:\n  - \"src/**\"\n---\n# style\n")
    write(home / ".claude" / "projects" / "repo" / "memory" / "MEMORY.md", "# index\n")
    write(home / ".claude" / "projects" / "repo" / "memory" / "feedback_tests.md",
          "---\ntype: feedback\nmodified: 2026-08-31T00:00:00Z\n---\nprefers pytest\n")
    write(home / ".claude" / "projects" / "repo" / "session.jsonl", "{}\n")
    write(home / ".codex" / "AGENTS.md", "# codex user\n")
    write(home / ".config" / "opencode" / "AGENTS.md", "# opencode global\n")
    write(home / ".pi" / "agent" / "SYSTEM.md", "# pi system\n")
    write(home / ".copilot" / "copilot-instructions.md", "# copilot user\n")
    write(home / ".copilot" / "instructions" / "team.instructions.md", "# copilot modular\n")
    write(home / ".gemini" / "GEMINI.md", "# antigravity global\n")
    write(home / ".gemini" / "antigravity-cli" / "plugins" / "demo" / "rules" / "one.md", "# plugin rule\n")
    (home / ".codex" / "memories_1.sqlite").write_bytes(b"SQLite format 3\x00")

def build_project(root: Path) -> None:
    (root / ".git").mkdir(parents=True, exist_ok=True)
    write(root / "AGENTS.md", "# shared agents\n")
    write(root / "CLAUDE.md", "@AGENTS.md\n\n# claude project\n")
    write(root / "CLAUDE.local.md", "# local only\n")
    write(root / ".claude" / "rules" / "api.md", "# api rules\n")
    write(root / ".agents" / "rules" / "workspace.md", "# antigravity workspace\n")
    write(root / ".github" / "copilot-instructions.md", "# repo copilot\n")
    write(root / "GEMINI.md", "# gemini project\n")

def payload(home: Path, project: Path, config: str = "", environ: dict | None = None,
            prefix: str = "") -> dict:
    variables = {"HOME": str(home)}
    if environ:
        variables.update(environ)
    return discovery.collect(str(project), str(home), config, environ=variables,
                             prefix=prefix, enforce_secure_system=False)

def by_name(document: dict, name: str) -> dict | None:
    for row in document["items"]:
        if row["name"] == name:
            return row
    return None

def agents_for(document: dict, name: str) -> list[str]:
    row = by_name(document, name)
    return sorted(reader["agent"] for reader in row["readers"]) if row else []

def test_schema_and_scopes(home: Path, project: Path) -> None:
    document = payload(home, project)
    assert document["schemaVersion"] == 1
    assert document["ok"] is True
    assert document["project"] == str(project)
    kinds = {row["kind"] for row in document["items"]}
    assert {"instructions", "rules", "auto-memory", "system-prompt"} <= kinds
    scopes = {row["scope"] for row in document["items"]}
    assert {"user", "project", "local", "plugin"} <= scopes

def test_scope_lanes_split_user_and_project(home: Path, project: Path) -> None:
    from fileblade_inventory import WatchPlan
    lanes = {}
    for scope in ("user", "project"):
        with WatchPlan() as plan:
            lanes[scope] = plan.finish(discovery.collect(str(project), str(home), "", environ={"HOME": str(home)},
                                                         enforce_secure_system=False, scope=scope))
    both = payload(home, project)
    user_scopes = {row["scope"] for row in lanes["user"]["items"]}
    assert not ({"project", "local"} & user_scopes), user_scopes
    assert {row["scope"] for row in lanes["project"]["items"]} == {"project", "local"}
    assert lanes["user"]["project"] == ""
    assert lanes["project"]["project"] == str(project)
    assert len(both["items"]) == len(lanes["user"]["items"]) + len(lanes["project"]["items"])
    inside = [str(path) for path in lanes["user"]["watchPaths"] if str(path).startswith(str(project))]
    assert not inside, inside
    user_roots = tuple(str(home / part) for part in (".copilot", ".pi", ".config/opencode",
                                                     ".gemini/antigravity-cli", ".claude/projects", ".claude/rules"))
    leaked = [str(path) for path in lanes["project"]["watchPaths"] if str(path).startswith(user_roots)]
    assert not leaked, leaked

def test_shared_reader_attribution(home: Path, project: Path) -> None:
    document = payload(home, project)
    assert agents_for(document, "AGENTS.md") == ["codex", "copilot-cli", "opencode", "pi"]
    assert "claude-code" not in agents_for(document, "AGENTS.md")
    claude_readers = agents_for(document, "CLAUDE.md")
    assert "claude-code" in claude_readers and "opencode" in claude_readers

def test_precedence_recorded(home: Path, project: Path) -> None:
    document = payload(home, project)
    row = by_name(document, "AGENTS.md")
    orders = {reader["agent"]: reader["loadOrder"] for reader in row["readers"]}
    assert orders["codex"] >= 20
    user_codex = by_name(document, "AGENTS.md")
    assert user_codex is not None
    assert all(reader["discovery"] == "documented" for reader in row["readers"])

def test_auto_memory_separated(home: Path, project: Path) -> None:
    document = payload(home, project)
    index = by_name(document, "MEMORY.md")
    topic = by_name(document, "feedback_tests.md")
    assert index["kind"] == "auto-memory" and "agent-written" in index["badges"]
    assert topic["memoryType"] == "feedback"
    assert topic["modified"] == "2026-08-31T00:00:00Z"

def test_private_stores_excluded(home: Path, project: Path) -> None:
    document = payload(home, project)
    names = {row["name"] for row in document["items"]}
    assert "memories_1.sqlite" not in names
    assert "session.jsonl" not in names
    assert "session-transcript" in document["excludedKinds"]

def test_no_filename_cross_products(home: Path, project: Path) -> None:
    write(home / ".claude" / "AGENTS.md", "# not read by claude\n")
    write(home / ".codex" / "CLAUDE.md", "# not read by codex\n")
    document = payload(home, project)
    for row in document["items"]:
        if row["path"].endswith(".claude/AGENTS.md"):
            raise AssertionError("claude home AGENTS.md must not be claimed")
        if row["path"].endswith(".codex/CLAUDE.md"):
            raise AssertionError("codex home CLAUDE.md must not be claimed")

def test_realpath_dedupe_with_aliases(home: Path, project: Path) -> None:
    target = write(home / "shared" / "rules.md", "# shared rule\n")
    link = home / ".claude" / "rules" / "linked.md"
    link.symlink_to(target)
    document = payload(home, project)
    rows = [row for row in document["items"] if row["realpath"] == str(target.resolve())]
    assert len(rows) == 1
    assert rows[0]["alias"] is True and "linked" in rows[0]["badges"]
    link.unlink()

def test_stable_ids(home: Path, project: Path) -> None:
    first = payload(home, project)
    second = payload(home, project)
    assert [row["id"] for row in first["items"]] == [row["id"] for row in second["items"]]
    row = by_name(first, "CLAUDE.md")
    assert row["id"] == common.stable_id(row["realpath"])

def test_extra_roots_configurable(home: Path, project: Path) -> None:
    extra = write(home / "vaultish" / "note.md", "# vault note\n")
    config = home / "config.json"
    config.write_text(json.dumps({"extraRoots": [{"path": str(extra.parent), "label": "vault"}]}), encoding="utf-8")
    document = payload(home, project, str(config))
    row = by_name(document, "note.md")
    assert row["kind"] == "extra" and row["scope"] == "extra"
    assert row["readers"][0]["discovery"] == "user-configured"
    plain = payload(home, project)
    assert by_name(plain, "note.md") is None

def test_bounded_descriptor_and_traversal(home: Path, project: Path) -> None:
    big = write(project / "BIG.md", "# head\n" + ("x" * (common.MAX_DESCRIPTOR_BYTES * 3)))
    deep = project / ".claude" / "rules"
    for level in range(common.MAX_RULE_DEPTH + 3):
        deep = deep / f"level{level}"
    write(deep / "deep.md", "# too deep\n")
    document = payload(home, project)
    assert by_name(document, "deep.md") is None
    write(project / "AGENTS.md", "# shared agents\n")
    assert big.stat().st_size > common.MAX_DESCRIPTOR_BYTES

def test_artifact_metrics_preserve_unicode_and_bounds(home: Path, project: Path) -> None:
    text = "# 記憶\n\n漢字 日本語 한국어 кириллица ελληνικά café\n"
    path = write(project / ".claude" / "rules" / "測試-память-μνήμη.md", text)
    modified = 1704067200
    os.utime(path, (modified, modified))

    metrics = common.artifact_metrics(path, text, path.stat().st_size)
    assert metrics["updated"] == dt.datetime.fromtimestamp(modified).strftime("%Y-%m-%d %H:%M")
    assert metrics["created"] == "" or re.fullmatch(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}", metrics["created"])
    assert metrics["bytes"] == len(text.encode("utf-8"))
    assert metrics["characters"] == len(text)
    assert metrics["words"] == len(re.findall(r"\w+", text, re.UNICODE))
    assert metrics["tokens"] == (len(text.encode("utf-8")) + 3) // 4

    row = by_name(payload(home, project), path.name)
    assert row is not None and row["metrics"] == metrics

    oversized_text = "# large\n" + "界" * common.MAX_DESCRIPTOR_BYTES
    oversized = write(project / ".claude" / "rules" / "大きい-файл.md", oversized_text)
    oversized_row = by_name(payload(home, project), oversized.name)
    assert oversized_row is not None
    assert oversized_row["metrics"]["bytes"] == oversized.stat().st_size
    for key in ("characters", "words", "tokens"):
        assert oversized_row["metrics"][key] is None

def test_irregular_files_ignored(home: Path, project: Path) -> None:
    fifo = project / ".claude" / "rules" / "pipe.md"
    fifo.parent.mkdir(parents=True, exist_ok=True)
    os.mkfifo(fifo)
    document = payload(home, project)
    assert by_name(document, "pipe.md") is None
    fifo.unlink()

def test_missing_home_is_empty(home: Path) -> None:
    with tempfile.TemporaryDirectory() as empty:
        document = discovery.collect("", str(Path(empty) / "nohome"), "", environ={})
        assert document["ok"] is True and document["count"] == 0

def test_codex_fallback_filenames(home: Path, project: Path) -> None:
    document = payload(home, project)
    contributing = by_name(document, "CONTRIBUTING.md")
    second = by_name(document, "ok.md")
    assert contributing is not None and second is not None
    readers = {reader["agent"] for reader in contributing["readers"]}
    assert readers == {"codex"}
    assert contributing["readers"][0]["discovery"] == "documented"
    assert contributing["note"] == "config fallback name"
    from agent_memory import adapters
    names = adapters.codex_fallback_names(home / ".codex")
    assert names == ["CONTRIBUTING.md", "ok.md", ".hidden"], names
    assert "../escape.md" not in names

def test_claude_auto_memory_directory_override(home: Path, project: Path) -> None:
    import shutil

    with tempfile.TemporaryDirectory() as raw:
        alternate = Path(raw) / "home"
        shutil.copytree(home, alternate, symlinks=True)
        write(alternate / ".claude" / "settings.json", json.dumps({
            "autoMemoryDirectory": str(alternate / "custom-memory"),
        }))
        write(alternate / "custom-memory" / "MEMORY.md", "# relocated index\n")
        write(alternate / "custom-memory" / "user_role.md", "---\ntype: user\n---\nprefers rust\n")
        document = discovery.collect(str(project), str(alternate), "", environ={"HOME": str(alternate)})
        index = by_name(document, "MEMORY.md")
        assert index is not None and index["path"].startswith(str(alternate / "custom-memory"))
        assert index["kind"] == "auto-memory"
        assert by_name(document, "user_role.md")["memoryType"] == "user"
        assert by_name(document, "feedback_tests.md") is None

def test_claude_md_excludes(home: Path, project: Path) -> None:
    document = payload(home, project)
    rule = by_name(document, "api.md")
    assert rule is not None
    assert rule["excluded"] is True and "excluded" in rule["badges"]
    assert all(reader["excluded"] for reader in rule["readers"])
    shared = by_name(document, "CLAUDE.md")
    assert shared is not None
    assert shared["excluded"] is False and "partly-excluded" in shared["badges"]
    flags = {reader["agent"]: reader["excluded"] for reader in shared["readers"]}
    assert flags["claude-code"] is True and flags["opencode"] is False
    other = by_name(document, "CLAUDE.local.md")
    assert other["excluded"] is False and "excluded" not in other["badges"]

def test_opencode_local_instructions_only(home: Path, project: Path) -> None:
    document = payload(home, project)
    guide = by_name(document, "local-guide.md")
    globbed = by_name(document, "one.md")
    assert guide is not None and globbed is not None
    assert {reader["agent"] for reader in guide["readers"]} == {"opencode"}
    assert by_name(document, "outside.md") is None
    assert by_name(document, "passwd") is None
    marker = [row for row in document["items"] if row["name"] == "opencode.json"]
    assert marker and marker[0]["note"].startswith("1 remote entries")
    assert marker[0]["detail"] == ""

def test_no_remote_fetch_and_no_config_contents(home: Path, project: Path) -> None:
    document = payload(home, project)
    blob = json.dumps(document)
    assert AUDIT_CANARY not in blob
    assert "example.invalid" not in blob
    assert "https://" not in blob

def test_copilot_custom_instruction_dirs(home: Path, project: Path) -> None:
    plain = payload(home, project)
    assert not any(row["path"].startswith(str(home / "copilot-extra")) for row in plain["items"])
    document = payload(home, project, environ={
        "COPILOT_CUSTOM_INSTRUCTIONS_DIRS": str(home / "copilot-extra") + ", , " + str(home / "missing"),
    })
    paths = {row["path"]: row for row in document["items"]}
    extra = paths.get(str(home / "copilot-extra" / "AGENTS.md"))
    assert extra is not None, sorted(path for path in paths if "copilot-extra" in path)
    assert {reader["agent"] for reader in extra["readers"]} == {"copilot-cli"}
    assert extra["note"] == "custom instructions dir"
    assert any(path.endswith("deep.instructions.md") for path in paths)
    assert not any(path.endswith("copilot-extra/extra.md") for path in paths)


def test_antigravity_global_context_is_one_row(home: Path, project: Path) -> None:
    write(home / ".gemini" / "GEMINI.md", "# shared global\n")
    document = payload(home, project)
    rows = [row for row in document["items"] if row["path"] == str(home / ".gemini" / "GEMINI.md")]
    assert len(rows) == 1
    agents = sorted(reader["agent"] for reader in rows[0]["readers"])
    assert agents == ["antigravity"], agents

def test_safe_basename_accepts_atypical_names(home: Path, project: Path) -> None:
    accepted = [
        "GEMINI.md", ".context.md", "notes..md", "..leading.md", "trailing..",
        "КОНТЕКСТ.md", "ΣΥΜΦΡΑΖΌΜΕΝΑ.md", "上下文.md", "한국어.md",
        "café.md", "cafe\u0301.md", "emoji-\U0001f600.md", "e\u0301\u0328.md",
        "name with spaces.md", "dash-and_underscore.md", "x" * common.MAX_BASENAME_CHARS,
    ]
    for name in accepted:
        assert common.safe_basename(name) == name, name

def test_safe_basename_rejects_hazards(home: Path, project: Path) -> None:
    rejected = [
        "", "   ", ".", "..", "a/b.md", "a\\b.md", "sub/dir.md", "../escape.md",
        "nul\x00.md", "bell\x07.md", "del\x7f.md", "c1\x9f.md",
        "bom\ufeff.md", "lrm\u200e.md", "zwsp\u200b.md" if unicodedata.category("\u200b") == "Cf" else "bom2\ufeff.md",
        "x" * (common.MAX_BASENAME_CHARS + 1), None, 42, ["GEMINI.md"],
    ]
    for name in rejected:
        assert common.safe_basename(name) == "", repr(name)
    surrogate = "lone" + chr(0xd800) + ".md"
    assert common.safe_basename(surrogate) == ""

def test_symlinked_rule_directory_is_not_traversed(home: Path, project: Path) -> None:
    outside = project.parent / "outside-rules"
    write(outside / "secret.md", "# outside rule\n")
    (project / ".claude" / "rules" / "linked").symlink_to(outside, target_is_directory=True)

    document = payload(home, project)
    paths = [row["path"] for row in document["items"]]
    assert not any("linked" in path for path in paths)
    assert not any(str(outside) in path for path in paths)
    (project / ".claude" / "rules" / "linked").unlink()
    shutil.rmtree(outside)

def test_symlinked_rule_root_is_not_traversed(home: Path, project: Path) -> None:
    outside = project.parent / "outside-rule-root"
    write(outside / "secret.md", "# outside root\n")
    linked = project.parent / "linked-rule-root"
    linked.symlink_to(outside, target_is_directory=True)
    assert common.walked_files(linked) == []
    linked.unlink()
    shutil.rmtree(outside)

def test_rule_directory_swap_is_not_traversed(home: Path, project: Path) -> None:
    root = project / ".claude" / "racing-rules"
    queued = root / "queued"
    moved = project.parent / "queued-original"
    outside = project.parent / "queued-outside"
    write(queued / "inside.md", "# inside\n")
    write(outside / "secret.md", "# outside\n")
    real_open = common.os.open
    swapped = False

    def racing_open(path: object, flags: int, mode: int = 0o777, *, dir_fd: int | None = None) -> int:
        nonlocal swapped
        if path == "queued" and dir_fd is not None and not swapped:
            swapped = True
            queued.rename(moved)
            queued.symlink_to(outside, target_is_directory=True)
        return real_open(path, flags, mode, dir_fd=dir_fd)

    common.os.open = racing_open
    try:
        assert common.walked_files(root) == []
    finally:
        common.os.open = real_open
        if queued.is_symlink():
            queued.unlink()
        shutil.rmtree(root)
        shutil.rmtree(moved)
        shutil.rmtree(outside)


def test_env_path_value_rules(home: Path, project: Path) -> None:
    assert common.env_path_value("X", {"X": "  spaced  "}) == "  spaced  "
    assert common.env_path_value("X", {"X": "   "}) == "   "
    assert common.env_path_value("X", {"X": "директория/файл"}) == "директория/файл"
    assert common.env_path_value("X", {}) == ""
    assert common.env_path_value("X", {"X": ""}) == ""
    assert common.env_path_value("X", {"X": "bad\x00path"}) == ""
    assert common.env_path_value("X", {"X": "x" * (common.MAX_ENV_PATH_CHARS + 1)}) == ""

def test_other_env_homes_preserve_spaces(home: Path, project: Path) -> None:
    codex_home = project.parent / "  codex  дом  "
    write(codex_home / "AGENTS.md", "# spaced codex home\n")
    document = payload(home, project, environ={"CODEX_HOME": str(codex_home)})
    assert str(codex_home / "AGENTS.md") in {row["path"] for row in document["items"]}

    copilot_dir = project.parent / " copilot  инструкции "
    write(copilot_dir / ".github" / "instructions" / "team.instructions.md", "# spaced copilot dir\n")
    document = payload(home, project, environ={"COPILOT_CUSTOM_INSTRUCTIONS_DIRS": str(copilot_dir)})
    assert str(copilot_dir / ".github" / "instructions" / "team.instructions.md") in {row["path"] for row in document["items"]}
    shutil.rmtree(codex_home)
    shutil.rmtree(copilot_dir)


def test_cli_contract(home: Path, project: Path) -> None:
    from agent_memory.cli import build_parser

    args = build_parser().parse_args(["list", "--project", str(project), "--home", str(home), "--json"])
    document = args.handler(args)
    assert document["schemaVersion"] == 1 and document["count"] > 0

def test_cli_output_bound(home: Path, project: Path) -> None:
    row = {"id": "x", "name": "entry", "path": "p" * 4096, "detail": "d" * 1024}
    raw = cli.encoded({"ok": True, "schemaVersion": 1, "truncated": False, "items": [row] * 1000})
    assert len(raw) <= cli.MAX_OUTPUT_BYTES
    document = json.loads(raw)
    assert document["truncated"] is True and document["count"] < 1000

def main() -> None:
    with tempfile.TemporaryDirectory() as sandbox:
        home = Path(sandbox) / "home"
        project = Path(sandbox) / "work" / "repo"
        build_home(home)
        build_project(project)
        build_audit_fixtures(home, project)
        checks = [
            test_schema_and_scopes, test_shared_reader_attribution, test_precedence_recorded,
            test_auto_memory_separated, test_private_stores_excluded, test_no_filename_cross_products,
            test_realpath_dedupe_with_aliases, test_stable_ids, test_extra_roots_configurable,
            test_bounded_descriptor_and_traversal, test_artifact_metrics_preserve_unicode_and_bounds,
            test_irregular_files_ignored,
            test_codex_fallback_filenames, test_claude_auto_memory_directory_override,
            test_claude_md_excludes, test_opencode_local_instructions_only,
            test_no_remote_fetch_and_no_config_contents, test_copilot_custom_instruction_dirs, test_antigravity_global_context_is_one_row,
            test_scope_lanes_split_user_and_project, test_safe_basename_accepts_atypical_names,
            test_safe_basename_rejects_hazards,
            test_symlinked_rule_directory_is_not_traversed,
            test_symlinked_rule_root_is_not_traversed, test_rule_directory_swap_is_not_traversed,
            test_env_path_value_rules, test_other_env_homes_preserve_spaces,
            test_cli_contract,
            test_cli_output_bound,
        ]
        passed = 0
        for check in checks:
            check(home, project)
            passed += 1
        test_missing_home_is_empty(home)
        passed += 1
    print(f"discovery tests: ok ({passed} checks)")

if __name__ == "__main__":
    main()
