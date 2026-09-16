from __future__ import annotations

from collections.abc import Callable
import json
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "python"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

import fixtures
from agent_hooks import apply, cli, discovery, events, safeio

ETC = {"root": ""}
TARGET_AGENTS = ("codex", "copilot-cli", "antigravity")

def environ(home: Path) -> dict[str, str]:
    return {"HOME": str(home)}

def inventory(home: Path, project: Path) -> dict:
    return discovery.collect(str(project), str(home), environ(home), ETC["root"], os.getuid())

def run(home: Path, project: Path, row_id: str, agents: list[str], state: str) -> dict:
    return apply.apply(str(project), row_id, agents, state, str(home), environ(home), ETC["root"], os.getuid())

def context(home: Path) -> dict:
    return {"home": home, "environ": environ(home)}

def find_row(home: Path, project: Path, **wanted) -> dict:
    for row in inventory(home, project)["items"]:
        if all(row.get(key) == value for key, value in wanted.items()):
            return row
    raise AssertionError(f"no row matching {wanted}")

def source_row(home: Path, project: Path) -> dict:
    document = inventory(home, project)
    for row in document["items"]:
        if row["agent"] == "claude-code" and row["scope"] == "user" and row["event"] == "PreToolUse" \
                and row["summary"]["timeoutSeconds"] == 30:
            return row
    raise AssertionError("claude PreToolUse fixture row missing")

def fresh(sandbox: Path) -> tuple[Path, Path]:
    home = sandbox / "home"
    project = sandbox / "work" / "repo"
    fixtures.build_all(home, project, None)
    for agent in TARGET_AGENTS:
        path = apply.target_path(agent, context(home))
        if path.exists():
            path.unlink()
    return home, project

def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))

def written(path: Path) -> dict:
    text = path.read_text(encoding="utf-8")
    assert text.endswith("\n") and (text == "{}\n" or text.startswith("{\n  ")), text[:40]
    assert "\t" not in text and "    \"" not in text.split("\n")[1]
    return json.loads(text)

def same_hook(home: Path, project: Path, agent: str, digest: str) -> dict:
    path = str(apply.target_path(agent, context(home)))
    for row in inventory(home, project)["items"]:
        if row["agent"] == agent and row["source"]["path"] == path and row["summary"]["digest"] == digest:
            assert row["scope"] == "user"
            return row
    raise AssertionError(f"{agent} has no row in {path} with digest {digest}")

def test_on_then_off_round_trip_per_agent(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = source_row(home, project)
    assert set(row["appliedAgents"]) == {"claude-code", "copilot-cli"}
    assert fixtures.CANARY_COMMAND in (home / ".copilot" / "hooks" / "guard.json").read_text(encoding="utf-8")
    for agent in TARGET_AGENTS:
        payload = run(home, project, row["id"], [agent], "on")
        assert payload["ok"] is True and payload["schemaVersion"] == 1
        outcome = payload["results"][0]
        path = apply.target_path(agent, context(home))
        assert outcome["agent"] == agent and outcome["ok"] and outcome["changed"], outcome
        assert outcome["touched"] == [str(path)]
        document = written(path)
        target_event = events.mapped_event("claude-code", "PreToolUse", agent)
        if agent == "antigravity":
            group = document["hook-" + row["summary"]["digest"]]
            entry = group[target_event][0]
            assert entry["matcher"] == "Bash"
            assert entry["hooks"][0] == {"type": "command", "command": fixtures.CANARY_COMMAND, "timeout": 30}
        elif agent == "copilot-cli":
            assert document["version"] == 1
            entry = document["hooks"][target_event][0]
            assert entry == {"type": "command", "bash": fixtures.CANARY_COMMAND, "timeoutSec": 30}
        else:
            entry = document["hooks"][target_event][0]
            assert entry["matcher"] == "Bash"
            hook = entry["hooks"][0]
            assert hook["command"] == fixtures.CANARY_COMMAND
            assert hook["timeout"] == 30
            assert "if" not in hook
        repeated = run(home, project, row["id"], [agent], "on")
        again = repeated["results"][0]
        assert repeated["ok"] is True and repeated["message"] == ""
        assert again["ok"] and not again["changed"] and again["touched"] == []
    after_on = find_row(home, project, id=row["id"])
    assert set(after_on["appliedAgents"]) == {"claude-code", *TARGET_AGENTS}
    for agent in TARGET_AGENTS:
        listed = same_hook(home, project, agent, row["summary"]["digest"])
        assert listed["appliedAgents"] == after_on["appliedAgents"]
        assert listed["summary"]["timeoutSeconds"] == 30
    for agent in TARGET_AGENTS:
        outcome = run(home, project, row["id"], [agent], "off")["results"][0]
        path = apply.target_path(agent, context(home))
        assert outcome["ok"] and outcome["changed"] and outcome["touched"] == [str(path)]
        document = written(path)
        assert row["summary"]["digest"] not in json.dumps(document)
        assert fixtures.CANARY_COMMAND not in json.dumps(document)
        if agent == "antigravity":
            assert "hook-" + row["summary"]["digest"] not in document
        else:
            assert document["hooks"] == {}
        repeat = run(home, project, row["id"], [agent], "off")["results"][0]
        assert repeat["ok"] and not repeat["changed"]
    assert set(find_row(home, project, id=row["id"])["appliedAgents"]) == {"claude-code", "copilot-cli"}

def test_condition_survives_only_where_supported(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = find_row(home, project, agent="claude-code", scope="user", event="PreToolUse", index=1)
    assert row["summary"]["hasCondition"] is True
    codex = run(home, project, row["id"], ["codex"], "on")["results"][0]
    assert codex["ok"] and codex["changed"]
    hook = written(apply.target_path("codex", context(home)))["hooks"]["PreToolUse"][0]["hooks"][0]
    assert hook["command"] == f"echo {fixtures.CANARY_TOKEN}" and "if" not in hook and "timeout" not in hook
    claude = run(home, project, row["id"], ["claude-code"], "on")["results"][0]
    assert claude["ok"] and not claude["changed"]

def test_existing_content_is_preserved(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = source_row(home, project)
    target = apply.target_path("codex", context(home))
    fixtures.write_json(target, {"model": "keep-me", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "stay"}]}]}})
    target.chmod(0o600)
    outcome = run(home, project, row["id"], ["codex"], "on")["results"][0]
    assert outcome["changed"], outcome
    document = written(target)
    assert document["model"] == "keep-me"
    assert document["hooks"]["Stop"][0]["hooks"][0]["command"] == "stay"
    assert document["hooks"]["PreToolUse"][0]["hooks"][0]["command"] == fixtures.CANARY_COMMAND
    assert oct(target.stat().st_mode & 0o777) == oct(0o600)
    assert not [name for name in os.listdir(target.parent) if name.endswith(".tmp")]

def test_event_mapping_refusals(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    unknown = find_row(home, project, agent="claude-code", event="NotARealEvent")
    for agent in TARGET_AGENTS:
        outcome = run(home, project, unknown["id"], [agent], "on")["results"][0]
        assert outcome["ok"] is False and outcome["changed"] is False
        assert "no event equivalent" in outcome["message"], outcome
        assert not apply.target_path(agent, context(home)).exists()
    invocation = find_row(home, project, agent="antigravity", event="PreInvocation")
    outcome = run(home, project, invocation["id"], ["claude-code"], "on")["results"][0]
    assert outcome["ok"] is False and "no event equivalent" in outcome["message"]
    assert events.mapped_event("claude-code", "PreCompact", "copilot-cli") == "preCompact"
    assert events.mapped_event("copilot-cli", "PreToolUse", "codex") == "PreToolUse"
    assert events.canonical("copilot-cli", "Stop") == "Stop"
    assert events.canonical("copilot-cli", "agentStop") == "Stop"
    assert events.canonical("antigravity", "PreInvocation") == "antigravity:PreInvocation"

def test_jsonc_target_is_refused_untouched(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = source_row(home, project)
    target = apply.target_path("codex", context(home))
    original = "// comment\n" + json.dumps({"hooks": {}}, indent=2) + "\n"
    fixtures.write_text(target, original)
    outcome = run(home, project, row["id"], ["codex"], "on")["results"][0]
    assert outcome["ok"] is False and outcome["changed"] is False
    assert "strict JSON" in outcome["message"]
    assert target.read_text(encoding="utf-8") == original
    trailing = json.dumps({"hooks": {}}, indent=2).replace("{}", "{},")
    fixtures.write_text(target, trailing)
    assert run(home, project, row["id"], ["codex"], "on")["results"][0]["ok"] is False
    assert target.read_text(encoding="utf-8") == trailing

def test_symlink_targets_are_updated_and_irregular_targets_refused(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = source_row(home, project)
    target = apply.target_path("codex", context(home))
    real = fixtures.write_json(home / "elsewhere" / "hooks.json", {"hooks": {}})
    target.parent.mkdir(parents=True, exist_ok=True)
    target.symlink_to(real)
    outcome = run(home, project, row["id"], ["codex"], "on")["results"][0]
    assert outcome["ok"] is True and outcome["changed"] is True
    assert target.is_symlink()
    assert json.loads(real.read_text(encoding="utf-8"))["hooks"]["PreToolUse"]
    target.unlink()
    fixtures.write_json(target, {"hooks": []})
    outcome = run(home, project, row["id"], ["codex"], "on")["results"][0]
    assert outcome["ok"] is False and "not an object" in outcome["message"]
    copilot = apply.target_path("copilot-cli", context(home))
    fixtures.write_json(copilot, {"version": 7, "hooks": {}})
    outcome = run(home, project, row["id"], ["copilot-cli"], "on")["results"][0]
    assert outcome["ok"] is False and "version" in outcome["message"]
    assert read_json(copilot) == {"version": 7, "hooks": {}}

def test_unknown_id_and_agents(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    payload = run(home, project, "not-a-row", ["codex"], "on")
    assert payload["ok"] is False and payload["results"] == []
    assert "no hook row" in payload["message"]
    row = source_row(home, project)
    payload = run(home, project, row["id"], ["cursor", "opencode", "pi"], "on")
    assert payload["ok"] is False
    assert payload["message"] == "; ".join(f"{outcome['agent']}: {outcome['message']}" for outcome in payload["results"])
    assert payload["message"].count("; ") == 2
    messages = {outcome["agent"]: outcome for outcome in payload["results"]}
    assert messages["cursor"]["ok"] is False and "unknown agent" in messages["cursor"]["message"]
    for code_hosted in ("opencode", "pi"):
        assert messages[code_hosted]["ok"] is False and "live in code" in messages[code_hosted]["message"]
    for agent in TARGET_AGENTS:
        assert not apply.target_path(agent, context(home)).exists()
    location = find_row(home, project, agent="opencode")
    payload = run(home, project, location["id"], ["codex"], "on")
    assert payload["ok"] is False and "code-hosted" in payload["message"]

def test_own_agent_off_is_refused(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = source_row(home, project)
    outcome = run(home, project, row["id"], ["claude-code"], "off")["results"][0]
    assert outcome["ok"] is False and "own agent" in outcome["message"]
    assert fixtures.CANARY_COMMAND in (home / ".claude" / "settings.json").read_text(encoding="utf-8")
    expanded = run(home, project, row["id"], ["all"], "off")
    assert [outcome["agent"] for outcome in expanded["results"]] == list(TARGET_AGENTS)
    everyone = run(home, project, row["id"], ["all", "codex"], "on")
    assert [outcome["agent"] for outcome in everyone["results"]] == ["claude-code", *TARGET_AGENTS]

def test_only_command_hooks_travel(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    prompt = find_row(home, project, agent="copilot-cli", event="postToolUse", scope="project")
    assert prompt["summary"]["type"] == "prompt"
    payload = run(home, project, prompt["id"], ["claude-code"], "on")
    assert payload["ok"] is False and "only command hooks" in payload["message"]
    bash = find_row(home, project, agent="copilot-cli", event="preToolUse", scope="user")
    payload = run(home, project, bash["id"], ["codex"], "on")
    assert payload["results"][0]["changed"], payload
    hook = written(apply.target_path("codex", context(home)))["hooks"]["PreToolUse"][0]
    assert hook["matcher"] == "shell"
    assert hook["hooks"][0] == {"type": "command", "command": fixtures.CANARY_COMMAND, "timeout": 15}

def test_antigravity_grouped_shape_is_listed(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    fixtures.write_json(home / ".gemini" / "config" / "hooks.json", {
        "my-linter-hook": {"PostToolUse": [{"matcher": "run_command",
                                            "hooks": [{"type": "command", "command": "./lint.sh", "timeout": 10}]}]},
        "safety-gate": {"enabled": False, "PreToolUse": [{"matcher": "run_command",
                                                          "hooks": [{"command": "./safety.sh"}]}]},
    })
    rows = [row for row in inventory(home, project)["items"] if row["agent"] == "antigravity" and row["scope"] == "user"]
    by_group = {row["group"]: row for row in rows}
    assert set(by_group) == {"my-linter-hook", "safety-gate"}
    assert by_group["my-linter-hook"]["summary"]["matcher"] == "literal"
    assert by_group["my-linter-hook"]["summary"]["timeoutSeconds"] == 10
    assert by_group["safety-gate"]["enabled"] is False
    assert len({row["id"] for row in rows}) == len(rows)

def test_top_level_ok_is_the_and_of_results(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = source_row(home, project)
    mixed = run(home, project, row["id"], ["codex", "opencode", "cursor"], "on")
    assert [outcome["ok"] for outcome in mixed["results"]] == [True, False, False]
    assert mixed["ok"] is False
    assert mixed["message"] == "opencode: opencode hooks live in code and have no hook file to write; cursor: unknown agent 'cursor'"
    clean = run(home, project, row["id"], ["codex", "copilot-cli"], "off")
    assert [outcome["ok"] for outcome in clean["results"]] == [True, True]
    assert clean["ok"] is True and clean["message"] == ""
    empty = run(home, project, row["id"], [], "on")
    assert empty["ok"] is False and empty["results"] == [] and empty["message"] == ""
    missing = run(home, project, "nope", ["codex"], "on")
    assert missing["ok"] is False and missing["results"] == [] and missing["message"].startswith("no hook row")

def test_cli_shape(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = source_row(home, project)
    import io
    captured = io.StringIO()
    original = sys.stdout
    sys.stdout = captured
    try:
        code = cli.main(["apply", "--project", str(project), "--home", str(home), "--id", row["id"],
                         "--agent", "codex", "--agent", "cursor", "--state", "on", "--json"])
    finally:
        sys.stdout = original
    payload = json.loads(captured.getvalue())
    assert code == 1 and payload["ok"] is False
    assert payload["message"].startswith("cursor: unknown agent")
    assert [outcome["agent"] for outcome in payload["results"]] == ["codex", "cursor"]
    assert [outcome["ok"] for outcome in payload["results"]] == [True, False]
    assert set(payload["results"][0]) == {"agent", "ok", "changed", "message", "touched"}
    captured = io.StringIO()
    sys.stdout = captured
    try:
        code = cli.main(["apply", "--project", str(project), "--home", str(home), "--id", row["id"],
                         "--agent", "copilot-cli", "--state", "on", "--json"])
    finally:
        sys.stdout = original
    payload = json.loads(captured.getvalue())
    assert code == 0 and payload["ok"] is True and payload["message"] == ""
    captured = io.StringIO()
    sys.stdout = captured
    try:
        code = cli.main(["apply", "--project", str(project), "--home", str(home), "--id", "missing",
                         "--agent", "codex", "--state", "off", "--json"])
    finally:
        sys.stdout = original
    assert code == 1 and json.loads(captured.getvalue())["ok"] is False

def test_remove_and_restore_round_trip(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = source_row(home, project)
    source = Path(row["source"]["path"])
    before = json.loads(source.read_text(encoding="utf-8"))
    removed = apply.remove(str(project), row["id"], str(home), environ(home), ETC["root"], os.getuid())
    assert removed["ok"], removed["message"]
    payload = removed["payload"]
    assert payload["agent"] == "claude-code" and payload["event"] == "PreToolUse" and payload["source"] == str(source)
    assert payload["format"] == 2 and payload["entry"]["command"] == fixtures.CANARY_COMMAND
    after = json.loads(source.read_text(encoding="utf-8"))
    assert after != before
    assert removed["results"][0]["changed"] is True
    assert not any(entry["summary"]["digest"] == row["summary"]["digest"] and entry["source"]["path"] == str(source)
                   for entry in inventory(home, project)["items"] if entry.get("summary"))
    again = apply.remove(str(project), "ffffffffffffffff", str(home), environ(home), ETC["root"], os.getuid())
    assert again["ok"] is False and "no hook row" in again["message"]
    import io
    arguments = ["restore", "--record-id", removed["recordId"], "--payload-stdin", "--json"]
    assert fixtures.CANARY_COMMAND not in " ".join(arguments)
    captured = io.StringIO()
    original_stdin = sys.stdin
    original_stdout = sys.stdout
    original_environment = dict(os.environ)
    sys.stdin = io.StringIO(json.dumps(payload) + "\n")
    sys.stdout = captured
    os.environ["HOME"] = str(home)
    os.environ.pop("XDG_STATE_HOME", None)
    try:
        code = cli.main(arguments)
    finally:
        sys.stdin = original_stdin
        sys.stdout = original_stdout
        os.environ.clear()
        os.environ.update(original_environment)
    restored = json.loads(captured.getvalue())
    assert code == 0
    assert fixtures.CANARY_COMMAND not in captured.getvalue()
    assert "payload" not in restored
    assert restored["ok"], restored["message"]
    assert any(entry["summary"]["digest"] == row["summary"]["digest"] and entry["event"] == "PreToolUse"
               for entry in inventory(home, project)["items"] if entry["agent"] == "claude-code")
    twice = apply.restore(removed["recordId"], json.dumps(payload), str(home), environ(home))
    assert twice["ok"] and twice["results"][0]["changed"] is False
    assert apply.restore("", "{}", str(home), environ(home))["ok"] is False
    assert apply.restore("", "nope", str(home), environ(home))["ok"] is False
    assert apply.restore("", json.dumps(dict(payload, agent="nobody")), str(home), environ(home))["ok"] is False
    assert read_json(source) == before

def test_remove_and_restore_preserve_a_symlinked_source(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    source = home / ".claude" / "settings.json"
    target = sandbox / "vault" / "settings.json"
    target.parent.mkdir(parents=True)
    source.rename(target)
    source.symlink_to(target)
    row = source_row(home, project)
    assert row["source"]["realpath"] == str(target)
    before = read_json(target)

    removed = apply.remove(str(project), row["id"], str(home), environ(home), ETC["root"], os.getuid())
    assert removed["ok"], removed["message"]
    assert source.is_symlink()
    assert read_json(target) != before
    restored = apply.restore(removed["recordId"], json.dumps(removed["payload"]), str(home), environ(home))
    assert restored["ok"], restored["message"]
    assert source.is_symlink()
    assert any(entry["summary"]["digest"] == row["summary"]["digest"]
               for entry in inventory(home, project)["items"] if entry["agent"] == "claude-code")

class BrokenSerializer:
    @staticmethod
    def dumps(*_args, **_kwargs) -> str:
        return '{"hooks": {"PreToolUse": ['

def refused(call: Callable[[], None]) -> str:
    try:
        call()
    except OSError as error:
        return str(error)
    raise AssertionError("the write was not refused")

def test_invalid_json_is_refused_and_the_file_is_untouched(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = source_row(home, project)
    target = apply.target_path("codex", context(home))
    fixtures.write_json(target, {"model": "keep-me", "hooks": {}})
    before = target.read_bytes()
    document = apply.load_target(target)
    assert not isinstance(document, str), document
    serializer = apply.json
    apply.json = BrokenSerializer
    try:
        message = refused(lambda: apply.write_atomic(target, document))
    finally:
        apply.json = serializer
    assert "does not parse as JSON" in message and "nothing was written" in message, message
    assert target.read_bytes() == before
    assert not [name for name in os.listdir(target.parent) if name.endswith(".tmp")]
    outcome = run(home, project, row["id"], ["codex"], "on")["results"][0]
    assert outcome["ok"] and outcome["changed"], outcome
    assert written(target)["model"] == "keep-me"

def test_dropping_an_unrelated_top_level_key_is_refused(sandbox: Path) -> None:
    home, project = fresh(sandbox)
    row = source_row(home, project)
    target = apply.target_path("codex", context(home))
    fixtures.write_json(target, {"model": "keep-me", "profile": {"name": "work"}, "hooks": {}})
    before = target.read_bytes()
    document = apply.load_target(target)
    document.pop("model")
    document.pop("profile")
    message = refused(lambda: apply.write_atomic(target, document))
    assert "would drop these top-level keys: model, profile" in message, message
    assert target.read_bytes() == before
    reshaped = apply.load_target(target)
    reshaped["profile"] = []
    assert "would change the type of these top-level keys: profile" in refused(
        lambda: apply.write_atomic(target, reshaped))
    assert target.read_bytes() == before
    declared = apply.load_target(target)
    declared.pop("profile")
    apply.write_atomic(target, declared, ("profile",))
    assert set(written(target)) == {"model", "hooks"}
    outcome = run(home, project, row["id"], ["codex"], "on")["results"][0]
    assert outcome["ok"] and outcome["changed"], outcome
    document = written(target)
    assert document["model"] == "keep-me"
    assert document["hooks"]["PreToolUse"][0]["hooks"][0]["command"] == fixtures.CANARY_COMMAND

def test_toml_sources_are_never_rewritten_as_json(sandbox: Path) -> None:
    home, _project = fresh(sandbox)
    source = home / ".codex" / "config.toml"
    before = source.read_bytes()
    message = refused(lambda: apply.write_atomic(source, {"hooks": {}}))
    assert "TOML hook files are never rewritten as JSON" in message, message
    assert source.read_bytes() == before
    linked = home / ".codex" / "linked.json"
    linked.symlink_to(source)
    assert "TOML hook files are never rewritten" in refused(lambda: apply.write_atomic(linked, {"hooks": {}}))
    assert source.read_bytes() == before
    assert safeio.document_kind(Path("a.json"), Path("b.toml")) == "toml"
    assert safeio.document_kind(Path("a.json")) == "json"

def test_the_guard_reads_each_format_as_itself(_sandbox: Path) -> None:
    toml_before = b'model = "gpt-5"\n\n[hooks]\nStop = []\n'
    assert safeio.refuse_update(toml_before, b'model = "gpt-5"\n[hooks]\nStop = ["x"]\n', "toml") == ""
    swallowed = safeio.refuse_update(toml_before, b'[hooks]\nStop = []\nmodel = "gpt-5"\n', "toml")
    assert "would drop these top-level keys: model" in swallowed, swallowed
    dropped = safeio.refuse_update(toml_before, b"[hooks]\nStop = []\n", "toml")
    assert "would drop these top-level keys: model" in dropped, dropped
    as_json = safeio.refuse_update(toml_before, b'{"model": "gpt-5", "hooks": {}}', "toml")
    assert "does not parse as TOML" in as_json, as_json
    json_before = b'{"model": "gpt-5", "hooks": {}}'
    assert safeio.refuse_update(json_before, b'{"hooks": {}, "model": "gpt-5"}', "json") == ""
    assert "does not parse as JSON" in safeio.refuse_update(json_before, b'model = "gpt-5"\n', "json")
    assert "does not parse as JSON" in safeio.refuse_update(json_before, b'{"model": "gpt-5", "hooks": {}', "json")
    assert "does not parse as JSON" in safeio.refuse_update(json_before, b"[1, 2]", "json")
    assert "does not parse as JSON" in safeio.refuse_update(json_before, b'{"model": "\xff"}', "json")
    assert safeio.refuse_update(None, b'{"hooks": {}}', "json") == ""
    assert "the file on disk is not valid JSON" in safeio.refuse_update(b"// jsonc\n{}", b'{"hooks": {}}', "json")
    deep = ('{"hooks": ' + "[" * 20 + "]" * 20 + "}").encode("utf-8")
    assert "nesting limit" in safeio.refuse_update(json_before, deep, "json")

def main() -> None:
    checks = [
        test_on_then_off_round_trip_per_agent, test_condition_survives_only_where_supported,
        test_existing_content_is_preserved, test_event_mapping_refusals, test_jsonc_target_is_refused_untouched,
        test_symlink_targets_are_updated_and_irregular_targets_refused, test_unknown_id_and_agents, test_own_agent_off_is_refused,
        test_only_command_hooks_travel,
        test_antigravity_grouped_shape_is_listed, test_top_level_ok_is_the_and_of_results, test_cli_shape,
        test_remove_and_restore_round_trip, test_remove_and_restore_preserve_a_symlinked_source,
        test_invalid_json_is_refused_and_the_file_is_untouched,
        test_dropping_an_unrelated_top_level_key_is_refused,
        test_toml_sources_are_never_rewritten_as_json, test_the_guard_reads_each_format_as_itself,
    ]
    passed = 0
    for check in checks:
        with tempfile.TemporaryDirectory() as raw:
            sandbox = Path(raw)
            ETC["root"] = str(sandbox / "etc")
            check(sandbox)
            passed += 1
    print(f"apply tests: ok ({passed} checks)")

if __name__ == "__main__":
    main()
