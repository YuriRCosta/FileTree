from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "python"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

import fixtures
from agent_hooks import discovery, safeio

ETC = {"root": ""}

def payload(home: Path, project: Path, environ: dict | None = None) -> dict:
    return discovery.collect(str(project), str(home), environ=environ or {"HOME": str(home)},
                             etc_root=ETC["root"] or "/etc", policy_owner_uid=os.getuid())

def rows_for(document: dict, agent: str) -> list[dict]:
    return [row for row in document["items"] if row["agent"] == agent]

def test_schema_and_agents(home: Path, project: Path) -> None:
    document = payload(home, project)
    assert document["schemaVersion"] == 1 and document["ok"] is True
    assert document["project"] == str(project)
    assert set(document["agents"]) == {
        "claude-code", "codex", "copilot-cli", "antigravity", "opencode", "pi",
    }
    assert document["agents"]["opencode"] == "code-hosted"
    assert document["agents"]["pi"] == "code-hosted"

def test_metrics_are_bounded_unicode_aware_and_dated(home: Path, project: Path) -> None:
    source = home / "atypical" / "挂钩-후크-κλειδί-хук.json"
    source.parent.mkdir(parents=True)
    text = "猫 κύμα Привет 한글"
    source.write_text(text, encoding="utf-8")
    stamp = 1_700_000_000
    os.utime(source, (stamp, stamp))

    metrics = safeio.artifact_metrics(source, text)
    encoded = text.encode("utf-8")
    assert metrics["updated"] == safeio.dt.datetime.fromtimestamp(stamp).strftime("%Y-%m-%d %H:%M")
    assert metrics["created"] == "" or len(metrics["created"]) == 16
    assert metrics["bytes"] == len(encoded)
    assert metrics["characters"] == len(text)
    assert metrics["words"] == 4
    assert metrics["tokens"] == (len(encoded) + 3) // 4

    oversized = safeio.artifact_metrics(source, "猫" * (safeio.MAX_FILE_BYTES + 1))
    assert oversized["bytes"] > safeio.MAX_FILE_BYTES
    assert oversized["characters"] is None and oversized["words"] is None
    assert oversized["tokens"] is None

    missing = safeio.artifact_metrics(source.with_name("失踪.json"), text)
    assert missing == {}

def test_discovered_metrics_count_redacted_payload_not_source(home: Path, project: Path) -> None:
    document = payload(home, project)
    row = next(item for item in document["items"]
               if item["supportStatus"] != "code-hosted" and item["summary"]["payloadBytes"] > 0)
    metrics = row["metrics"]
    assert metrics["bytes"] == row["summary"]["payloadBytes"]
    assert metrics["characters"] >= 1 and metrics["words"] >= 1 and metrics["tokens"] >= 1
    assert isinstance(metrics["updated"], str) and len(metrics["updated"]) == 16
    assert metrics["created"] == "" or len(metrics["created"]) == 16

def test_every_documented_agent_produces_rows(home: Path, project: Path) -> None:
    document = payload(home, project)
    for agent in ("claude-code", "codex", "copilot-cli", "antigravity"):
        found = rows_for(document, agent)
        assert found, agent
        assert all(row["supportStatus"] in ("documented", "undocumented-event") for row in found), agent

def test_exact_root_skips_the_walk(home: Path, project: Path) -> None:
    nested = project / "packages" / "web"
    nested.mkdir(parents=True, exist_ok=True)
    common = {"environ": {"HOME": str(home)}, "etc_root": ETC["root"] or "/etc", "policy_owner_uid": os.getuid()}
    walked = discovery.collect(str(nested), str(home), **common)
    exact = discovery.collect(str(nested), str(home), exact=True, **common)
    assert walked["project"] == str(project)
    assert exact["project"] == str(nested)

def test_codex_hooks_json_events_live_at_the_root(home: Path, project: Path) -> None:
    document = payload(home, project)
    codex = rows_for(document, "codex")
    user_stop = [row for row in codex if row["event"] == "Stop" and row["scope"] == "user"]
    assert user_stop, [row["event"] + ":" + row["scope"] for row in codex]
    wrapped = [row for row in codex if row["event"] == "SubagentStop" and row["scope"] == "project"]
    assert wrapped, "a hooks wrapper is still tolerated"

def test_scope_lanes_split_user_and_project(home: Path, project: Path) -> None:
    from fileblade_inventory import WatchPlan
    common = {"environ": {"HOME": str(home)}, "etc_root": ETC["root"] or "/etc", "policy_owner_uid": os.getuid()}
    lanes = {}
    for scope in ("user", "project"):
        with WatchPlan() as plan:
            lanes[scope] = plan.finish(discovery.collect(str(project), str(home), scope=scope, **common))
    both = discovery.collect(str(project), str(home), **common)
    user_scopes = {row["scope"] for row in lanes["user"]["items"]}
    assert not ({"project", "local"} & user_scopes), user_scopes
    assert {row["scope"] for row in lanes["project"]["items"]} == {"project", "local"}
    assert lanes["user"]["project"] == ""
    assert lanes["project"]["project"] == str(project)
    assert len(both["items"]) == len(lanes["user"]["items"]) + len(lanes["project"]["items"])
    inside = [str(path) for path in lanes["user"]["watchPaths"] if str(path).startswith(str(project))]
    assert not inside, inside
    user_roots = tuple(str(home / part) for part in (".copilot", ".pi", ".claude/plugins",
                                                     ".config/opencode", ".gemini/config"))
    leaked = [str(path) for path in lanes["project"]["watchPaths"] if str(path).startswith(user_roots)]
    assert not leaked, leaked

def test_scopes_and_events(home: Path, project: Path) -> None:
    document = payload(home, project)
    claude = rows_for(document, "claude-code")
    assert {row["scope"] for row in claude} >= {"user", "project", "local", "plugin"}
    assert any(row["event"] == "PreToolUse" for row in claude)
    assert any(row["supportStatus"] == "undocumented-event" and row["event"] == "NotARealEvent" for row in claude)

def test_enabled_and_disabled_states(home: Path, project: Path) -> None:
    document = payload(home, project)
    antigravity = rows_for(document, "antigravity")
    assert any(row["enabled"] is True for row in antigravity)
    assert any(row["enabled"] is False for row in antigravity)
    copilot = rows_for(document, "copilot-cli")
    assert any(row["enabled"] is False and "file-disabled" in row["badges"] for row in copilot)
    assert any(row["enabled"] is None for row in copilot)

def test_codex_documented_sources_only(home: Path, project: Path) -> None:
    document = payload(home, project)
    codex = rows_for(document, "codex")
    assert codex
    assert all("trust" not in row for row in codex)
    events = {row["event"] for row in codex}
    assert {"Stop", "PreCompact", "SubagentStop"} <= events, events
    scopes = {row["scope"] for row in codex}
    assert {"user", "project"} <= scopes, scopes
    assert all(row["supportStatus"] == "documented" for row in codex)
    assert not any("feature-off" in row["badges"] for row in codex)

def test_codex_feature_flag_is_reported(home: Path, project: Path) -> None:
    import shutil
    import tempfile
    with tempfile.TemporaryDirectory() as raw:
        alternate = Path(raw) / "home"
        shutil.copytree(home, alternate)
        config = alternate / ".codex" / "config.toml"
        config.write_text(config.read_text().replace("hooks = true", "hooks = false"), encoding="utf-8")
        document = discovery.collect(str(project), str(alternate), environ={"HOME": str(alternate)})
        codex = [row for row in document["items"] if row["agent"] == "codex" and row["scope"] != "profile"]
        assert codex and all("feature-off" in row["badges"] for row in codex)

def test_claude_full_event_table(home: Path, project: Path) -> None:
    from agent_hooks import adapters
    assert "InstructionsLoaded" in adapters.CLAUDE_EVENTS
    assert len(adapters.CLAUDE_EVENTS) == 33
    assert len(adapters.CODEX_EVENTS) == 12
    assert "Interrupt" in adapters.CODEX_EVENTS

def test_copilot_rejects_bad_version(home: Path, project: Path) -> None:
    document = payload(home, project)
    blob = safeio.serialized(document)
    assert "REJECTED-BAD-VERSION" not in blob
    copilot = rows_for(document, "copilot-cli")
    assert any(row["event"] == "subagentStart" for row in copilot)

def test_undocumented_plugin_hook_path_is_not_scanned(home: Path, project: Path) -> None:
    document = payload(home, project)
    blob = safeio.serialized(document)
    assert "antigravity-cli" not in blob
    assert "UNDOCUMENTED-PLUGIN-PATH" not in blob

def test_stdout_is_bounded_to_one_mebibyte(home: Path, project: Path) -> None:
    document = payload(home, project)
    template = document["items"][0] if document["items"] else {"id": "x", "note": ""}
    bulk = dict(document)
    bulk["items"] = [dict(template, id=f"row-{index:06d}", note="N" * 400) for index in range(6000)]
    bulk["count"] = len(bulk["items"])
    bulk["truncated"] = False
    assert len(safeio.serialized(bulk).encode("utf-8")) > safeio.MAX_STDOUT_BYTES
    text = safeio.fit_payload(bulk)
    assert len(text.encode("utf-8")) <= safeio.MAX_STDOUT_BYTES
    reparsed = json.loads(text)
    assert reparsed["truncated"] is True
    assert 0 < reparsed["count"] < 6000
    assert reparsed["count"] == len(reparsed["items"])
    small = payload(home, project)
    assert json.loads(safeio.fit_payload(small))["count"] == small["count"]

def test_code_hosted_agents_are_locations_only(home: Path, project: Path) -> None:
    document = payload(home, project)
    for agent in ("opencode", "pi"):
        found = rows_for(document, agent)
        assert found, agent
        for row in found:
            assert row["supportStatus"] == "code-hosted"
            assert row["event"] == ""
            assert row["summary"]["type"] == "code"
            assert row["summary"]["digest"] == ""
            assert "not-inspected" in row["badges"]
            assert row["entries"] >= 1

def test_unsupported_locations_are_not_guessed(home: Path, project: Path) -> None:
    document = payload(home, project)
    paths = " ".join(row["source"]["path"] for row in document["items"])
    assert ".cursor" not in paths
    assert "credentials" not in paths
    assert ".netrc" not in paths

def test_stable_ids(home: Path, project: Path) -> None:
    first = payload(home, project)
    second = payload(home, project)
    assert [row["id"] for row in first["items"]] == [row["id"] for row in second["items"]]
    assert len({row["id"] for row in first["items"]}) == len(first["items"])

def test_symlinked_config_resolves_to_its_target(home: Path, project: Path) -> None:
    target = fixtures.write_json(home / "elsewhere" / "hooks.json", {
        "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "linked"}]}]},
    })
    link = home / ".codex" / "linked-hooks.json"
    real = home / ".codex" / "hooks.json"
    backup = real.with_suffix(".bak")
    real.rename(backup)
    real.symlink_to(target)
    document = payload(home, project)
    linked = [row for row in rows_for(document, "codex") if row["source"]["path"] == str(real)]
    assert linked and linked[0]["event"] == "Stop"
    assert linked[0]["source"]["realpath"] == str(target)
    assert [row for row in rows_for(document, "codex") if row["scope"] == "project"]
    real.unlink()
    backup.rename(real)
    if link.exists():
        link.unlink()

def test_bounds_are_enforced(home: Path, project: Path) -> None:
    oversize = home / ".claude" / "big.json"
    oversize.parent.mkdir(parents=True, exist_ok=True)
    oversize.write_bytes(b"{" + b" " * (safeio.MAX_FILE_BYTES + 10) + b"}")
    assert safeio.read_bytes(oversize) is None
    deep: object = "leaf"
    for _ in range(safeio.MAX_JSON_DEPTH + 3):
        deep = {"nested": deep}
    assert safeio.bounded_depth(deep) is False
    oversize.unlink()

def test_irregular_files_are_skipped(home: Path, project: Path) -> None:
    fifo = home / ".copilot" / "hooks" / "pipe.json"
    fifo.parent.mkdir(parents=True, exist_ok=True)
    os.mkfifo(fifo)
    document = payload(home, project)
    assert all(not row["source"]["path"].endswith("pipe.json") for row in document["items"])
    fifo.unlink()

def test_malformed_files_do_not_break_the_scan(home: Path, project: Path) -> None:
    broken = home / ".copilot" / "hooks" / "broken.json"
    broken.write_text("{ not json at all", encoding="utf-8")
    document = payload(home, project)
    assert document["ok"] is True and document["count"] > 0
    broken.unlink()

def test_missing_home_is_empty() -> None:
    with tempfile.TemporaryDirectory() as sandbox:
        document = discovery.collect("", str(Path(sandbox) / "nothing"), environ={})
        assert document["ok"] is True and document["count"] == 0

def copilot_rows(document: dict, scope: str) -> list[dict]:
    return [row for row in rows_for(document, "copilot-cli") if row["scope"] == scope]

def test_copilot_policy_files_are_ordered_and_gated(home: Path, project: Path) -> None:
    from agent_hooks import adapters
    context = {
        "home": home, "projectRoot": str(project), "environ": {"HOME": str(home)},
        "etcRoot": Path(ETC["root"]), "policyOwnerUid": os.getuid(),
    }
    read_order = [Path(row["source"]["path"]).name
                  for row in adapters.copilot_policy_rows(context, safeio.Budget())]
    assert read_order == sorted(read_order), read_order
    assert read_order[0] == "10-first.json", read_order
    document = payload(home, project)
    policy = copilot_rows(document, "policy")
    assert {row["event"] for row in policy} == {"sessionStart", "preToolUse"}, policy
    blob = safeio.serialized(document)
    assert "POLICY-BAD-VERSION" not in blob
    assert "POLICY-GROUP-WRITABLE" not in blob
    assert all("policy" in row["badges"] for row in policy)

def test_policy_rejects_wrong_owner(home: Path, project: Path) -> None:
    document = discovery.collect(str(project), str(home), environ={"HOME": str(home)},
                                 etc_root=ETC["root"] or "/etc", policy_owner_uid=os.getuid() + 1)
    assert not copilot_rows(document, "policy")

def test_copilot_standalone_files_keep_the_version_gate(home: Path, project: Path) -> None:
    document = payload(home, project)
    blob = safeio.serialized(document)
    assert "REJECTED-BAD-VERSION" not in blob
    user = [row for row in copilot_rows(document, "user") if "settings-inline" not in row["badges"]]
    assert any(row["event"] == "preToolUse" for row in user)
    off = [row for row in user if "file-disabled" in row["badges"]]
    assert off and all(row["enabled"] is False for row in off)
    assert any(row["enabled"] is not False for row in user)

def test_copilot_settings_need_no_version(home: Path, project: Path) -> None:
    document = payload(home, project)
    inline = [row for row in rows_for(document, "copilot-cli") if "settings-inline" in row["badges"]]
    events = {row["event"] for row in inline}
    assert {"userPromptSubmitted", "subagentStart", "PreToolUse"} <= events, events
    assert all(row["supportStatus"] == "documented" for row in inline)
    assert not any("version" in row for row in inline)
    shared = {row["event"] for row in inline if "/.claude/" in row["source"]["path"]}
    assert {"Stop", "PostToolUse"} <= shared, shared

def test_copilot_plugin_layouts(home: Path, project: Path) -> None:
    document = payload(home, project)
    plugins = copilot_rows(document, "plugin")
    assert {row["plugin"] for row in plugins} == {"guard-plugin", "direct-plugin"}
    assert all(row["enabled"] is None for row in plugins)
    assert all("enable-state-undocumented" in row["badges"] for row in plugins)

def test_pascal_aliases_are_documented(home: Path, project: Path) -> None:
    from agent_hooks import adapters
    assert len(adapters.COPILOT_EVENTS) == 14
    assert len(adapters.COPILOT_PASCAL_ALIASES) == 12
    assert set(adapters.COPILOT_PASCAL_ALIASES.values()) <= set(adapters.COPILOT_EVENTS)
    document = payload(home, project)
    aliased = [row for row in rows_for(document, "copilot-cli") if "vscode-alias" in row["badges"]]
    assert aliased
    for row in aliased:
        assert row["supportStatus"] == "documented"
        assert row["canonicalEvent"] in adapters.COPILOT_EVENTS

def test_repository_disable_all_hooks_spares_policy(home: Path, project: Path) -> None:
    import shutil
    import tempfile
    with tempfile.TemporaryDirectory() as raw:
        alternate = Path(raw) / "repo"
        shutil.copytree(project, alternate)
        fixtures.write_json(alternate / ".github" / "copilot" / "settings.json",
                            {"disableAllHooks": True})
        document = discovery.collect(str(alternate), str(home), environ={"HOME": str(home)},
                                     etc_root=ETC["root"] or "/etc", policy_owner_uid=os.getuid())
        copilot = rows_for(document, "copilot-cli")
        policy = [row for row in copilot if row["scope"] == "policy"]
        others = [row for row in copilot if row["scope"] != "policy"]
        assert policy and all(row["enabled"] is not False for row in policy)
        assert others and all(row["enabled"] is False for row in others)
        assert all("repository-disabled" in row["badges"] for row in others)

def test_profile_rows_never_carry_base_feature_off(home: Path, project: Path) -> None:
    import shutil
    import tempfile
    with tempfile.TemporaryDirectory() as raw:
        alternate = Path(raw) / "home"
        shutil.copytree(home, alternate)
        config = alternate / ".codex" / "config.toml"
        config.write_text(config.read_text().replace("hooks = true", "hooks = false"), encoding="utf-8")
        document = discovery.collect(str(project), str(alternate), environ={"HOME": str(alternate)},
                                     etc_root=ETC["root"] or "/etc", policy_owner_uid=os.getuid())
        codex = rows_for(document, "codex")
        active = [row for row in codex if row["scope"] != "profile"]
        profiles = [row for row in codex if row["scope"] == "profile"]
        assert active and all("feature-off" in row["badges"] for row in active)
        assert profiles, "profile fixtures did not produce rows"
        assert all("feature-off" not in row["badges"] for row in profiles)
        assert all(row["enabled"] is None for row in profiles)
        assert all("profile-unselected" in row["badges"] for row in profiles)
        assert all(row["note"] == "applies only when codex runs with --profile" for row in profiles)

def test_codex_profiles_are_never_active(home: Path, project: Path) -> None:
    document = payload(home, project)
    profiles = [row for row in rows_for(document, "codex") if row["scope"] == "profile"]
    assert {row["profile"] for row in profiles} == {"tûrkçe-pröfil", "plain"}, profiles
    assert all(row["enabled"] is None for row in profiles)
    assert all("profile-unselected" in row["badges"] for row in profiles)
    assert all(row["supportStatus"] == "documented" for row in profiles)
    unicode_row = [row for row in profiles if row["profile"] == "tûrkçe-pröfil"][0]
    assert "tûrkçe-pröfil.config.toml" in unicode_row["source"]["path"]
    assert unicode_row["source"]["name"] == "tûrkçe-pröfil.config.toml"
    active = [row for row in rows_for(document, "codex") if row["scope"] in ("user", "project")]
    assert active and all(row["scope"] != "profile" for row in active)
    assert not any("profile-unselected" in row["badges"] for row in active)

def test_codex_config_toml_is_not_a_profile(home: Path, project: Path) -> None:
    from agent_hooks import adapters
    names = [path.name for path in adapters.codex_profile_paths(home / ".codex")]
    assert "config.toml" not in names
    assert sorted(names) == sorted(["plain.config.toml", "tûrkçe-pröfil.config.toml"]), names

def test_labels_come_from_status_message_then_script_basename(home: Path, project: Path) -> None:
    import json
    settings = home / ".claude" / "settings.json"
    original = settings.read_text(encoding="utf-8")
    document = json.loads(original)
    document["hooks"]["PreCompact"] = [{"hooks": [
        {"type": "command", "command": "/home/someone/.claude/hooks/tidy-context.sh --fast", "statusMessage": "  Tidying   context "},
        {"type": "command", "command": "~/.claude/hooks/guard-secrets.py"},
        {"type": "command", "command": "bun test"},
    ]}]
    settings.write_text(json.dumps(document), encoding="utf-8")
    try:
        rows = [row for row in rows_for(payload(home, project), "claude-code") if row["event"] == "PreCompact"]
        labels = {row["summary"]["label"]: row["summary"]["labelSource"] for row in rows}
        assert labels == {"Tidying context": "statusMessage", "guard-secrets.py": "command", "": ""}, labels
        text = json.dumps(rows)
        assert "/home/someone" not in text and "--fast" not in text and "bun test" not in text
    finally:
        settings.write_text(original, encoding="utf-8")

def test_user_labels_override_and_clear(home: Path, project: Path) -> None:
    from agent_hooks import labels
    environ = {"HOME": str(home)}
    row = rows_for(payload(home, project, environ=environ), "claude-code")[0]
    digest = row["summary"]["digest"]
    result = labels.set_label(home, environ, digest, "  Block dangerous  shell ")
    assert result["ok"] and result["changed"] and result["label"] == "Block dangerous shell"
    assert result["touched"] == [str(home / ".config" / "omarchy" / "fileblade" / "hooks" / "labels.json")]
    relisted = [item for item in rows_for(payload(home, project, environ=environ), "claude-code") if item["id"] == row["id"]][0]
    assert relisted["summary"]["label"] == "Block dangerous shell" and relisted["summary"]["labelSource"] == "user"
    again = labels.set_label(home, environ, digest, "Block dangerous shell")
    assert again["ok"] and not again["changed"] and again["touched"] == []
    assert not labels.set_label(home, environ, "not-hex!", "x")["ok"]
    cleared = labels.set_label(home, environ, digest, "")
    assert cleared["ok"] and cleared["changed"]
    restored = [item for item in rows_for(payload(home, project, environ=environ), "claude-code") if item["id"] == row["id"]][0]
    assert restored["summary"]["labelSource"] != "user"
    (home / ".config" / "omarchy" / "fileblade" / "hooks" / "labels.json").unlink()

def main() -> None:
    checks = [
        test_schema_and_agents, test_metrics_are_bounded_unicode_aware_and_dated,
        test_labels_come_from_status_message_then_script_basename, test_user_labels_override_and_clear,
        test_discovered_metrics_count_redacted_payload_not_source,
        test_every_documented_agent_produces_rows, test_exact_root_skips_the_walk, test_scopes_and_events,
        test_enabled_and_disabled_states, test_codex_documented_sources_only,
        test_codex_feature_flag_is_reported, test_claude_full_event_table,
        test_copilot_rejects_bad_version, test_undocumented_plugin_hook_path_is_not_scanned,
        test_stdout_is_bounded_to_one_mebibyte, test_code_hosted_agents_are_locations_only,
        test_codex_hooks_json_events_live_at_the_root, test_scope_lanes_split_user_and_project,
        test_unsupported_locations_are_not_guessed, test_stable_ids, test_symlinked_config_resolves_to_its_target,
        test_bounds_are_enforced, test_irregular_files_are_skipped, test_malformed_files_do_not_break_the_scan,
        test_copilot_policy_files_are_ordered_and_gated, test_policy_rejects_wrong_owner,
        test_copilot_standalone_files_keep_the_version_gate, test_copilot_settings_need_no_version,
        test_copilot_plugin_layouts, test_pascal_aliases_are_documented,
        test_repository_disable_all_hooks_spares_policy,
        test_codex_profiles_are_never_active, test_codex_config_toml_is_not_a_profile,
        test_profile_rows_never_carry_base_feature_off,
    ]
    with tempfile.TemporaryDirectory() as sandbox:
        home = Path(sandbox) / "home"
        project = Path(sandbox) / "work" / "repo"
        etc = Path(sandbox) / "etc"
        ETC["root"] = str(etc)
        fixtures.build_all(home, project, etc)
        passed = 0
        for check in checks:
            check(home, project)
            passed += 1
    test_missing_home_is_empty()
    print(f"discovery tests: ok ({passed + 1} checks)")

if __name__ == "__main__":
    main()
