from __future__ import annotations

import contextlib
import io
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "python"))

from agent_memory import apply, cli, discovery

ALL_AGENTS = ["claude-code", "codex", "opencode", "pi", "copilot-cli", "antigravity"]
DIRECT_PROJECT_READERS = {"codex", "opencode", "pi", "copilot-cli"}

def write(path: Path, text: str = "# heading\n\nbody\n") -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path

def environ_for(home: Path) -> dict[str, str]:
    return {"HOME": str(home)}

def listing(home: Path, project: Path) -> dict:
    return discovery.collect(str(project), str(home), "", environ=environ_for(home), enforce_secure_system=False)

def row_for(home: Path, project: Path, realpath: Path) -> dict:
    for row in listing(home, project)["items"]:
        if row["realpath"] == str(realpath.resolve()):
            return row
    raise AssertionError(f"no row for {realpath}")

def readers_of(home: Path, project: Path, realpath: Path) -> set[str]:
    return {reader["agent"] for reader in row_for(home, project, realpath)["readers"]}

def run(home: Path, project: Path, row_id: str, agents: list[str], state: str, **extra) -> dict:
    variables = environ_for(home)
    variables.update(extra.pop("environ", {}))
    return apply.apply(str(project), row_id, agents, state, str(home), "", variables,
                       enforce_secure_system=False, **extra)

def results_by_agent(document: dict) -> dict[str, dict]:
    return {entry["agent"]: entry for entry in document["results"]}

def snapshot(base: Path) -> list[tuple[str, str, int]]:
    rows = []
    for current, directories, files in os.walk(base):
        for name in sorted(directories) + sorted(files):
            path = Path(current) / name
            metadata = path.lstat()
            rows.append((str(path), oct(metadata.st_mode), metadata.st_mtime_ns))
    return sorted(rows)

def fresh(sandbox: Path, name: str) -> tuple[Path, Path]:
    home = sandbox / name / "home"
    project = sandbox / name / "work" / "repo"
    (project / ".git").mkdir(parents=True)
    home.mkdir(parents=True)
    return home, project

def test_project_round_trip_every_agent(sandbox: Path) -> None:
    home, project = fresh(sandbox, "project-round-trip")
    source = write(project / "AGENTS.md", "# shared instructions\n")
    row = row_for(home, project, source)
    assert set(reader["agent"] for reader in row["readers"]) == DIRECT_PROJECT_READERS
    on = run(home, project, row["id"], ALL_AGENTS, "on")
    assert on["ok"] is True and on["schemaVersion"] == 1 and on["project"] == str(project)
    by_agent = results_by_agent(on)
    for agent in DIRECT_PROJECT_READERS:
        assert by_agent[agent]["ok"] and not by_agent[agent]["changed"], agent
        assert by_agent[agent]["touched"] == []
    for agent, link in (("claude-code", project / "CLAUDE.md"),
                        ("antigravity", project / ".agents" / "rules" / "AGENTS.md")):
        assert by_agent[agent]["changed"] is True, agent
        assert by_agent[agent]["touched"] == [str(link)]
        assert link.is_symlink() and link.resolve() == source.resolve(), agent
        assert os.readlink(link) == str(source.resolve())
    assert readers_of(home, project, source) == set(ALL_AGENTS)
    assert row_for(home, project, source)["id"] == row["id"]
    off = run(home, project, row["id"], ALL_AGENTS, "off")
    assert off["ok"] is True, off["message"]
    by_agent = results_by_agent(off)
    for agent in ("claude-code", "antigravity"):
        assert by_agent[agent]["changed"] is True, agent
    for agent in DIRECT_PROJECT_READERS:
        assert by_agent[agent]["ok"] and not by_agent[agent]["changed"], agent
    assert source.is_file() and not source.is_symlink()
    assert source.read_text(encoding="utf-8") == "# shared instructions\n"
    assert not (project / "CLAUDE.md").exists() and not (project / "GEMINI.md").exists()
    assert not (project / ".agents" / "rules" / "AGENTS.md").exists()
    assert readers_of(home, project, source) == DIRECT_PROJECT_READERS

def test_user_round_trip_every_agent(sandbox: Path) -> None:
    home, project = fresh(sandbox, "user-round-trip")
    source = write(home / ".claude" / "CLAUDE.md", "# user instructions\n")
    row = row_for(home, project, source)
    assert row["scope"] == "user"
    for missing in (".codex", ".pi", ".copilot", ".gemini", ".config"):
        assert not (home / missing).exists(), missing
    on = run(home, project, row["id"], ALL_AGENTS, "on")
    assert on["ok"] is True, on["message"]
    expected = {
        "codex": home / ".codex" / "AGENTS.md",
        "opencode": home / ".config" / "opencode" / "AGENTS.md",
        "pi": home / ".pi" / "agent" / "AGENTS.md",
        "copilot-cli": home / ".copilot" / "copilot-instructions.md",
        "antigravity": home / ".gemini" / "GEMINI.md",
    }
    by_agent = results_by_agent(on)
    for agent, link in expected.items():
        assert by_agent[agent]["changed"] is True, agent
        assert link.is_symlink() and link.resolve() == source.resolve(), agent
    assert by_agent["claude-code"]["ok"] and not by_agent["claude-code"]["changed"]
    assert readers_of(home, project, source) == set(ALL_AGENTS)
    off = run(home, project, row["id"], ALL_AGENTS, "off")
    assert off["ok"] is True, off["message"]
    by_agent = results_by_agent(off)
    assert by_agent["antigravity"]["changed"] is True
    for link in expected.values():
        assert not link.exists() and not link.is_symlink()
    assert source.is_file() and source.read_text(encoding="utf-8") == "# user instructions\n"
    assert readers_of(home, project, source) == {"claude-code", "opencode"}

def test_already_linked_is_idempotent(sandbox: Path) -> None:
    home, project = fresh(sandbox, "idempotent")
    source = write(project / "AGENTS.md")
    row = row_for(home, project, source)
    first = results_by_agent(run(home, project, row["id"], ["claude-code"], "on"))["claude-code"]
    second = results_by_agent(run(home, project, row["id"], ["claude-code"], "on"))["claude-code"]
    assert first["changed"] is True
    assert second["ok"] is True and second["changed"] is False and second["touched"] == []
    assert (project / "CLAUDE.md").is_symlink()
    twice_off = run(home, project, row["id"], ["claude-code"], "off")
    again = run(home, project, row["id"], ["claude-code"], "off")
    assert results_by_agent(twice_off)["claude-code"]["changed"] is True
    assert results_by_agent(again)["claude-code"] == {
        "agent": "claude-code", "ok": True, "changed": False,
        "message": f"nothing linked at {project / 'CLAUDE.md'}", "touched": [],
    }

def test_real_file_and_foreign_link_are_refused(sandbox: Path) -> None:
    home, project = fresh(sandbox, "refusals")
    source = write(project / "AGENTS.md", "# agents\n")
    real = write(project / "CLAUDE.md", "# separate claude file\n")
    other = write(project / "docs" / "other.md", "# other\n")
    foreign = project / ".agents" / "rules" / "AGENTS.md"
    foreign.parent.mkdir(parents=True)
    foreign.symlink_to(other)
    row = row_for(home, project, source)
    document = run(home, project, row["id"], ["claude-code", "antigravity"], "on")
    assert document["ok"] is False
    by_agent = results_by_agent(document)
    assert by_agent["claude-code"]["ok"] is False and by_agent["claude-code"]["changed"] is False
    assert "real file" in by_agent["claude-code"]["message"]
    assert by_agent["antigravity"]["ok"] is False and "different file" in by_agent["antigravity"]["message"]
    assert "claude-code:" in document["message"] and "antigravity:" in document["message"]
    assert real.read_text(encoding="utf-8") == "# separate claude file\n" and not real.is_symlink()
    assert foreign.resolve() == other.resolve()
    off = run(home, project, row["id"], ["claude-code", "antigravity", "codex"], "off")
    assert off["ok"] is True
    assert real.is_file() and foreign.is_symlink() and source.is_file()
    for entry in off["results"]:
        assert entry["changed"] is False and entry["touched"] == []

def test_rules_and_other_kinds_are_refused(sandbox: Path) -> None:
    home, project = fresh(sandbox, "kinds")
    rule = write(project / ".claude" / "rules" / "api.md", "# api rule\n")
    memory = write(home / ".claude" / "projects" / "repo" / "memory" / "MEMORY.md", "# index\n")
    system = write(home / ".pi" / "agent" / "SYSTEM.md", "# system\n")
    before = snapshot(project.parent.parent)
    for path in (rule, memory, system):
        row = row_for(home, project, path)
        document = run(home, project, row["id"], ["codex", "claude-code"], "on")
        assert document["ok"] is False
        for entry in document["results"]:
            assert entry["ok"] is False and entry["changed"] is False and entry["touched"] == []
            assert row["kind"] in entry["message"]
    assert snapshot(project.parent.parent) == before

def test_unknown_row_and_agent(sandbox: Path) -> None:
    home, project = fresh(sandbox, "unknown")
    source = write(project / "AGENTS.md")
    row = row_for(home, project, source)
    missing = run(home, project, "deadbeefdeadbeef", ["codex"], "on")
    assert missing["ok"] is False
    assert missing["results"][0]["ok"] is False and "deadbeefdeadbeef" in missing["results"][0]["message"]
    bogus = run(home, project, row["id"], ["codex", "not-an-agent"], "on")
    assert bogus["ok"] is False
    by_agent = results_by_agent(bogus)
    assert by_agent["codex"]["ok"] is True
    assert by_agent["not-an-agent"]["ok"] is False and "unknown agent" in by_agent["not-an-agent"]["message"]
    assert bogus["message"] == "not-an-agent: unknown agent id 'not-an-agent'"
    bad_state = run(home, project, row["id"], ["codex"], "sideways")
    assert bad_state["ok"] is False and "state" in bad_state["results"][0]["message"]

def test_source_behind_symlink_links_to_realpath(sandbox: Path) -> None:
    home, project = fresh(sandbox, "vault")
    vault = write(sandbox / "vault" / "instructions.md", "# vault copy\n")
    alias = project / "AGENTS.md"
    alias.symlink_to(vault)
    row = row_for(home, project, vault)
    assert row["alias"] is True
    document = run(home, project, row["id"], ["claude-code", "antigravity"], "on")
    assert document["ok"] is True, document["message"]
    assert os.readlink(project / "CLAUDE.md") == str(vault.resolve())
    assert (project / ".agents" / "rules" / "instructions.md").is_symlink()
    off = run(home, project, row["id"], ["claude-code", "antigravity", "codex"], "off")
    assert off["ok"] is True
    by_agent = results_by_agent(off)
    assert by_agent["claude-code"]["changed"] and by_agent["antigravity"]["changed"]
    assert by_agent["codex"]["changed"] is True and not alias.exists()
    assert vault.is_file() and vault.read_text(encoding="utf-8") == "# vault copy\n"

def test_environment_relocations_are_honoured(sandbox: Path) -> None:
    home, project = fresh(sandbox, "relocated")
    source = write(home / ".claude" / "CLAUDE.md")
    codex_home = sandbox / "relocated" / "codex home"
    config_home = sandbox / "relocated" / "xdg"
    copilot_home = sandbox / "relocated" / "copilot"
    row = row_for(home, project, source)
    document = run(home, project, row["id"], ["codex", "opencode", "copilot-cli", "antigravity"], "on",
                   environ={"CODEX_HOME": str(codex_home), "XDG_CONFIG_HOME": str(config_home),
                            "COPILOT_HOME": str(copilot_home)})
    assert document["ok"] is True, document["message"]
    for link in (codex_home / "AGENTS.md", config_home / "opencode" / "AGENTS.md",
                 copilot_home / "copilot-instructions.md",
                 home / ".gemini" / "GEMINI.md"):
        assert link.is_symlink() and link.resolve() == source.resolve(), link
    assert not (home / ".codex").exists() and not (home / ".copilot").exists()

def test_list_writes_nothing(sandbox: Path) -> None:
    home, project = fresh(sandbox, "read-only")
    write(project / "AGENTS.md")
    write(project / "CLAUDE.md")
    write(home / ".claude" / "CLAUDE.md")
    write(home / ".gemini" / "GEMINI.md")
    before = snapshot(sandbox / "read-only")
    for _ in range(2):
        listing(home, project)
        args = cli.build_parser().parse_args(["list", "--project", str(project), "--home", str(home), "--json"])
        args.handler(args)
    assert snapshot(sandbox / "read-only") == before

def test_cli_apply_shape(sandbox: Path) -> None:
    home, project = fresh(sandbox, "cli")
    source = write(project / "AGENTS.md")
    row = row_for(home, project, source)
    parser = cli.build_parser()
    args = parser.parse_args(["apply", "--project", str(project), "--home", str(home), "--id", row["id"],
                              "--agent", "claude-code", "--agent", "antigravity", "--state", "on", "--json"])
    assert args.emitter is cli.emit_results
    document = args.handler(args)
    assert set(document) == {"ok", "schemaVersion", "project", "message", "results"}
    assert [entry["agent"] for entry in document["results"]] == ["claude-code", "antigravity"]
    for entry in document["results"]:
        assert set(entry) == {"agent", "ok", "changed", "message", "touched"}
    raw = cli.encoded_results(document)
    assert json.loads(raw)["ok"] is True and len(raw) <= cli.MAX_OUTPUT_BYTES
    oversized = dict(document)
    oversized["results"] = [{"agent": "x", "ok": True, "changed": False, "message": "m" * 4096, "touched": []}] * 400
    bounded = json.loads(cli.encoded_results(oversized))
    assert bounded["ok"] is False and bounded["results"] == []
    for bad in (["apply", "--id", "x", "--state", "on"], ["apply", "--id", "x", "--agent", "codex"],
                ["apply", "--agent", "codex", "--state", "on"], ["apply", "--id", "x", "--agent", "codex", "--state", "maybe"]):
        with contextlib.redirect_stderr(io.StringIO()):
            try:
                parser.parse_args(bad)
            except SystemExit as exit_code:
                assert exit_code.code == 2
            else:
                raise AssertionError(f"{bad} should be rejected")

def main() -> None:
    checks = [
        test_project_round_trip_every_agent, test_user_round_trip_every_agent, test_already_linked_is_idempotent,
        test_real_file_and_foreign_link_are_refused, test_rules_and_other_kinds_are_refused,
        test_unknown_row_and_agent, test_source_behind_symlink_links_to_realpath,
        test_environment_relocations_are_honoured, test_list_writes_nothing, test_cli_apply_shape,
    ]
    with tempfile.TemporaryDirectory() as raw:
        sandbox = Path(raw)
        for check in checks:
            check(sandbox)
            shutil.rmtree(sandbox / check.__name__.replace("test_", "").replace("_", "-"), ignore_errors=True)
    print(f"apply tests: ok ({len(checks)} checks)")

if __name__ == "__main__":
    main()
