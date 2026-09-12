from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "python"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from agent_skills import apply, bounds, cli, discovery, frontmatter, registry
import fixtures
from fixtures import link_skill, write_skill

RESULTS: list[tuple[str, bool, str]] = []

def check(name: str, condition: bool, detail: str = "") -> None:
    RESULTS.append((name, bool(condition), detail))

def env_for(home: Path, anchor: Path | str = "", prefix: Path | str = "", platform: str = "linux",
            environ: dict | None = None, enforce_secure_system: bool = False):
    return discovery.Environment(
        home=str(home),
        anchor=str(anchor),
        platform=platform,
        prefix=str(prefix) if prefix else "",
        environ=dict(environ or {}),
        enforce_secure_system=enforce_secure_system,
    )

def names(payload) -> set[str]:
    return {row["name"] for row in payload["items"]}

def row_for(payload, name: str):
    for row in payload["items"]:
        if row["name"] == name:
            return row
    return None

def test_frontmatter() -> None:
    folded = "---\nname: demo\ndescription: >\n  A wrapped\n  description here\n---\nbody\n"
    check("frontmatter.name", frontmatter.value(folded, "name") == "demo")
    check("frontmatter.folded", frontmatter.value(folded, "description") == "A wrapped description here")
    check("frontmatter.absent", frontmatter.value("no frontmatter", "name") == "")
    nested = "---\nmeta:\n  description: nested\nname: real\n---\n"
    check("frontmatter.nested-ignored", frontmatter.value(nested, "description") == "")
    check("frontmatter.quotes", frontmatter.value('---\nname: "q" # c\n---\n', "name") == "q")
    unterminated = "---\nname: never\n" + ("x\n" * 10)
    check("frontmatter.unterminated", frontmatter.value(unterminated, "name") == "")

def test_bounds(tmp: Path) -> None:
    big = tmp / "big.md"
    big.write_text("x" * (bounds.MAX_DESCRIPTOR_BYTES + 4096), encoding="utf-8")
    check("bounds.oversize-refused", bounds.read_descriptor(str(big)) is None)
    small = tmp / "small.md"
    small.write_text("hello", encoding="utf-8")
    check("bounds.regular-read", bounds.read_descriptor(str(small)) == b"hello")
    fifo = tmp / "fifo.md"
    os.mkfifo(fifo)
    check("bounds.fifo-refused", bounds.read_descriptor(str(fifo)) is None)
    link = tmp / "link.md"
    os.symlink(small, link)
    check("bounds.symlink-descriptor-refused", bounds.read_descriptor(str(link)) is None)
    check("bounds.control-stripped", bounds.clean("a\x00b\x07c", 64) == "abc")
    check("bounds.length-capped", len(bounds.clean("y" * 900, 512)) == 512)

def test_artifact_metrics(tmp: Path) -> None:
    text = "汉字 日本語 한국어 Кириллица Ελληνικά café 😀"
    descriptor = tmp / "技能-навык-δεξιότητα.md"
    descriptor.write_text(text, encoding="utf-8")
    description = "汉字 日本語"
    metrics = bounds.artifact_metrics(str(descriptor), text, description)
    check("metrics.updated", bool(metrics.get("updated")), str(metrics))
    check("metrics.created-shape", isinstance(metrics.get("created"), str), str(metrics))
    check("metrics.bytes", metrics.get("bytes") == len(text.encode("utf-8")), str(metrics))
    check("metrics.characters-unicode", metrics.get("characters") == len(text), str(metrics))
    check("metrics.words-unicode", metrics.get("words") == 6, str(metrics))
    check("metrics.tokens-description", metrics.get("tokens") == (len(description.encode("utf-8")) + 3) // 4,
          str(metrics))
    check("metrics.file-tokens", metrics.get("fileTokens") == (len(text.encode("utf-8")) + 3) // 4, str(metrics))
    check("metrics.tokens-empty-description", bounds.artifact_metrics(str(descriptor), text).get("tokens") == 0)

def test_uppercase_only(tmp: Path) -> None:
    home = tmp / "home"
    root = home / ".claude" / "skills"
    write_skill(root, "good")
    lower = root / "lower"
    lower.mkdir(parents=True)
    (lower / "skill.md").write_text("---\nname: lower\n---\n", encoding="utf-8")
    payload = discovery.collect(env_for(home))
    check("uppercase.accepts-SKILL.md", "good" in names(payload))
    check("uppercase.rejects-skill.md", "lower" not in names(payload))

def test_documented_roots_only(tmp: Path) -> None:
    home = tmp / "home"
    write_skill(home / ".claude" / "skills", "claude-user")
    write_skill(home / ".agents" / "skills", "agents-user")
    write_skill(home / ".pi" / "agent" / "skills", "pi-user")
    write_skill(home / ".copilot" / "skills", "copilot-user")
    write_skill(home / ".config" / "opencode" / "skills", "opencode-user")
    write_skill(home / ".gemini" / "antigravity-cli" / "skills", "antigravity-cli")
    write_skill(home / ".gemini" / "antigravity" / "skills", "antigravity-undocumented")
    write_skill(home / ".agent" / "skills", "antigravity-legacy-undocumented")
    write_skill(home / ".codex" / "skills", "codex-undocumented")
    write_skill(home / ".invented" / "skills", "invented")
    write_skill(home / ".claude" / "skill", "singular")
    payload = discovery.collect(env_for(home))
    found = names(payload)
    for expected in (
        "claude-user", "agents-user", "pi-user", "copilot-user",
        "opencode-user", "antigravity-cli",
    ):
        check(f"roots.documented:{expected}", expected in found)
    check("roots.rejects-dot-codex-skills", "codex-undocumented" not in found)
    check("roots.rejects-antigravity-ide-guess", "antigravity-undocumented" not in found)
    check("roots.rejects-antigravity-legacy-guess", "antigravity-legacy-undocumented" not in found)
    check("roots.rejects-invented-dotdir", "invented" not in found)
    check("roots.rejects-singular-skill", "singular" not in found)

def test_shared_reader_attribution(tmp: Path) -> None:
    home = tmp / "home"
    write_skill(home / ".agents" / "skills", "shared")
    write_skill(home / ".claude" / "skills", "claude-only")
    payload = discovery.collect(env_for(home))
    shared = row_for(payload, "shared")
    claude_only = row_for(payload, "claude-only")
    expected = {"codex", "opencode", "pi", "copilot-cli"}
    check("attribution.shared-readers", set(shared["agents"]) == expected, str(shared["agents"]))
    check("attribution.claude-not-in-agents-skills", "claude-code" not in shared["agents"])
    check("attribution.claude-skills", set(claude_only["agents"]) == {"claude-code", "opencode"},
          str(claude_only["agents"]))

def test_symlink_dedupe(tmp: Path) -> None:
    home = tmp / "home"
    vault = tmp / "vault"
    descriptor = write_skill(vault, "linked")
    link_skill(home / ".claude" / "skills", "linked", descriptor.parent)
    link_skill(home / ".agents" / "skills", "linked", descriptor.parent)
    payload = discovery.collect(env_for(home))
    rows = [row for row in payload["items"] if row["name"] == "linked"]
    check("dedupe.single-row", len(rows) == 1, f"{len(rows)} rows")
    check("dedupe.merges-agents", len(rows[0]["agents"]) == 5, str(rows[0]["agents"]))
    check("dedupe.canonical-path", rows[0]["path"].startswith(str(home / ".claude")), rows[0]["path"])
    check("dedupe.link-target", rows[0]["link_target"] == str(descriptor))

def test_scope_not_merged(tmp: Path) -> None:
    home = tmp / "home"
    project = tmp / "project"
    (project / ".git").mkdir(parents=True)
    vault = tmp / "vault2"
    descriptor = write_skill(vault, "both")
    link_skill(home / ".claude" / "skills", "both", descriptor.parent)
    link_skill(project / ".claude" / "skills", "both", descriptor.parent)
    payload = discovery.collect(env_for(home, project))
    scopes = sorted(row["scope"] for row in payload["items"] if row["name"] == "both")
    check("scope.user-and-project-kept", scopes == ["project", "user"], str(scopes))

def test_scope_lanes(tmp: Path) -> None:
    from fileblade_inventory import WatchPlan
    home = tmp / "home"
    project = tmp / "project"
    (project / ".git").mkdir(parents=True)
    write_skill(home / ".claude" / "skills", "user-only")
    write_skill(project / ".claude" / "skills", "project-only")
    env = env_for(home, project)
    both = discovery.collect(env)
    check("scope.all-has-both", {"user-only", "project-only"} <= names(both), str(names(both)))
    lanes = {}
    for scope in ("user", "project"):
        lane_env = discovery.Environment(home=env.home, anchor=env.anchor, platform=env.platform, prefix=env.prefix,
                                         environ=env.environ, enforce_secure_system=env.enforce_secure_system, scope=scope)
        with WatchPlan() as plan:
            lanes[scope] = plan.finish(discovery.collect(lane_env))
    check("scope.user-lane-rows", names(lanes["user"]) == {"user-only"}, str(names(lanes["user"])))
    check("scope.project-lane-rows", names(lanes["project"]) == {"project-only"}, str(names(lanes["project"])))
    check("scope.project-lane-scope", {row["scope"] for row in lanes["project"]["items"]} == {"project"})
    check("scope.lanes-partition-all", len(both["items"]) == len(lanes["user"]["items"]) + len(lanes["project"]["items"]))
    inside = [str(path) for path in lanes["user"]["watchPaths"] if str(path).startswith(str(project))]
    check("scope.user-lane-never-watches-project", not inside, str(inside))

def test_exact_project(tmp: Path) -> None:
    home = tmp / "home"
    repo = tmp / "repo"
    nested = repo / "packages" / "web"
    nested.mkdir(parents=True)
    (repo / ".git").mkdir()
    write_skill(nested / ".claude" / "skills", "nested-only")
    env = env_for(home, nested)
    exact = discovery.Environment(home=env.home, anchor=env.anchor, exact=True, platform=env.platform,
                                  prefix=env.prefix, environ=env.environ, enforce_secure_system=env.enforce_secure_system)
    payload = discovery.collect(exact)
    check("exact.project-is-anchor", payload["project"] == str(nested), payload["project"])
    check("exact.nested-skill", "nested-only" in names(payload))
    walked = discovery.collect(env)
    check("exact.walk-differs", walked["project"] == str(repo), walked["project"])

def test_project_walk(tmp: Path) -> None:
    home = tmp / "home"
    repo = tmp / "repo"
    nested = repo / "packages" / "web"
    nested.mkdir(parents=True)
    (repo / ".git").mkdir()
    write_skill(repo / ".claude" / "skills", "repo-root")
    write_skill(nested / ".claude" / "skills", "nested-package")
    payload = discovery.collect(env_for(home, nested))
    found = names(payload)
    check("walk.project-root", payload["project"] == str(repo), payload["project"])
    check("walk.repo-root-skill", "repo-root" in found)
    check("walk.nested-skill", "nested-package" in found)
    bare = tmp / "bare" / "deep"
    bare.mkdir(parents=True)
    write_skill(bare / ".claude" / "skills", "unbounded")
    markers = registry.PROJECT_MARKERS
    registry.PROJECT_MARKERS = (".fileblade-skills-no-such-marker",)
    try:
        outside = discovery.collect(env_for(home, bare))
        chain = discovery.project_chain(str(bare))[1]
    finally:
        registry.PROJECT_MARKERS = markers
    check("walk.non-repository-anchor-is-workspace", outside["project"] == str(bare), str(outside["project"]))
    check("walk.non-repository-parent-not-walked", chain == [str(bare)], chain)
    check("walk.collects-every-level", "unbounded" in names(outside))
    check("walk.empty-anchor", discovery.project_chain("") == ("", []))
    check("walk.bounded", len(discovery.project_chain(str(bare))[1]) <= bounds.MAX_PROJECT_WALK)

def test_managed_and_system(tmp: Path) -> None:
    home = tmp / "home"
    prefix = tmp / "prefix"
    write_skill(prefix / "etc" / "claude-code" / ".claude" / "skills", "managed-skill")
    write_skill(prefix / "etc" / "codex" / "skills", "codex-system")
    payload = discovery.collect(env_for(home, prefix=prefix))
    managed = row_for(payload, "managed-skill")
    system = row_for(payload, "codex-system")
    check("managed.found", managed is not None and managed["scope"] == "managed")
    check("managed.precedence", managed is not None and managed["precedence"] == 0)
    check("system.codex-etc", system is not None and system["scope"] == "system")
    darwin = discovery.collect(env_for(home, prefix=prefix, platform="darwin"))
    check("managed.platform-specific", row_for(darwin, "managed-skill") is None)

def test_reserved_synced(tmp: Path) -> None:
    home = tmp / "home"
    write_skill(home / ".claude" / "skills" / "synced", "from-claude-ai")
    write_skill(home / ".agents" / "skills" / "synced", "not-reserved-here")
    payload = discovery.collect(env_for(home))
    synced = row_for(payload, "from-claude-ai")
    check("synced.recursed", synced is not None)
    check("synced.source", synced is not None and synced["source"] == "synced")
    check("synced.only-claude-roots", row_for(payload, "not-reserved-here") is None)

def test_plugins_and_enable_state(tmp: Path) -> None:
    home = tmp / "home"
    cache = home / ".claude" / "plugins" / "cache" / "market" / "demo"
    write_skill(cache / "1.0.0" / "skills", "installed-skill")
    write_skill(cache / "0.9.0" / "skills", "stale-skill")
    single = home / ".claude" / "plugins" / "cache" / "market" / "solo" / "1.0.0"
    single.mkdir(parents=True)
    (single / "SKILL.md").write_text("---\nname: solo-root\ndescription: one\n---\n", encoding="utf-8")
    plugins = home / ".claude" / "plugins"
    plugins.mkdir(parents=True, exist_ok=True)
    (plugins / "installed_plugins.json").write_text(json.dumps({
        "version": 2,
        "plugins": {
            "demo@market": [{"installPath": str(cache / "1.0.0"), "version": "1.0.0"}],
            "solo@market": [{"installPath": str(single), "version": "1.0.0"}],
        },
    }), encoding="utf-8")
    (home / ".claude").mkdir(parents=True, exist_ok=True)
    (home / ".claude" / "settings.json").write_text(json.dumps({
        "enabledPlugins": {"demo@market": True, "solo@market": False},
    }), encoding="utf-8")
    payload = discovery.collect(env_for(home))
    installed = row_for(payload, "installed-skill")
    solo = row_for(payload, "solo-root")
    check("plugin.installed-version-only", installed is not None)
    check("plugin.stale-version-skipped", row_for(payload, "stale-skill") is None)
    check("plugin.source-label", installed is not None and installed["source"] == "plugin:demo@market")
    check("plugin.enabled-true", installed is not None and installed["enabled"] is True)
    check("plugin.root-skill-md", solo is not None)
    check("plugin.enabled-false", solo is not None and solo["enabled"] is False)
    check("plugin.disabled-badge", solo is not None and "disabled" in solo["badges"])

def test_codex_switches(tmp: Path) -> None:
    home = tmp / "home"
    descriptor = write_skill(home / ".agents" / "skills", "switched")
    (home / ".codex").mkdir(parents=True, exist_ok=True)
    (home / ".codex" / "config.toml").write_text(
        f'[[skills.config]]\npath = "{descriptor}"\nenabled = false\n', encoding="utf-8")
    payload = discovery.collect(env_for(home))
    row = row_for(payload, "switched")
    check("codex.enable-switch", row is not None and row["enabled"] is False)
    broken = tmp / "broken"
    (broken / ".codex").mkdir(parents=True)
    (broken / ".codex" / "config.toml").write_text("not = [valid toml\n", encoding="utf-8")
    check("codex.malformed-toml-safe", discovery.codex_switches(env_for(broken)) == {})

def test_output_bounds(tmp: Path) -> None:
    home = tmp / "home"
    root = home / ".claude" / "skills"
    write_skill(root, "huge", "D" * 4000)
    write_skill(root, "overcap", "E" * (bounds.MAX_FRONTMATTER_BYTES + 4096))
    payload = discovery.collect(env_for(home))
    row = row_for(payload, "huge")
    check("output.detail-capped", row is not None and len(row["detail"]) == bounds.MAX_DETAIL,
          "" if row is None else str(len(row["detail"])))
    overcap = row_for(payload, "overcap")
    check("output.overcap-frontmatter-degrades", overcap is not None and overcap["detail"] == "")
    for index in range(bounds.MAX_ITEMS + 20):
        write_skill(root, f"bulk-{index:04d}")
    capped = discovery.collect(env_for(home))
    check("output.item-cap", capped["count"] == bounds.MAX_ITEMS, str(capped["count"]))
    check("output.truncated-flag", capped["truncated"] is True)
    hostile = dict(capped)
    hostile["items"] = [dict(capped["items"][0], roots=["界" * 4096] * 8) for _ in range(bounds.MAX_ITEMS)]
    serialized = cli.encoded(hostile)
    check("output.serialized-byte-cap", len(serialized) <= cli.MAX_OUTPUT_BYTES, str(len(serialized)))
    check("output.serialized-valid-json", isinstance(json.loads(serialized)["items"], list))

def test_stable_ids(tmp: Path) -> None:
    home = tmp / "home"
    write_skill(home / ".claude" / "skills", "stable")
    first = discovery.collect(env_for(home))
    second = discovery.collect(env_for(home))
    check("ids.stable-across-runs", row_for(first, "stable")["id"] == row_for(second, "stable")["id"])
    check("ids.unique", len({row["id"] for row in first["items"]}) == len(first["items"]))

def test_hostile_inputs(tmp: Path) -> None:
    home = tmp / "home"
    root = home / ".claude" / "skills"
    root.mkdir(parents=True)
    escaped = root / "escape"
    escaped.mkdir()
    (escaped / "SKILL.md").write_text(
        "---\nname: bad\x07name\ndescription: <b>markup</b> and \x00 nul\n---\n", encoding="utf-8")
    dangling = root / "dangling"
    os.symlink(tmp / "nowhere", dangling)
    fifo_dir = root / "fifo"
    fifo_dir.mkdir()
    os.mkfifo(fifo_dir / "SKILL.md")
    payload = discovery.collect(env_for(home))
    row = row_for(payload, "badname")
    check("hostile.control-chars-stripped", row is not None, str(names(payload)))
    check("hostile.detail-kept-plain", row is not None and "\x00" not in row["detail"])
    check("hostile.dangling-symlink-skipped", not any(r["name"] == "dangling" for r in payload["items"]))
    check("hostile.fifo-descriptor-skipped", not any(r["name"] == "fifo" for r in payload["items"]))

def test_empty_home(tmp: Path) -> None:
    payload = discovery.collect(env_for(tmp / "absent-home"))
    check("empty.ok", payload["ok"] is True)
    check("empty.no-items", payload["count"] == 0)
    check("empty.schema", payload["schemaVersion"] == 1)

def test_registry_shape() -> None:
    check("registry.agent-count", len(registry.AGENT_LABELS) == 6)
    check("registry.every-root-documented", all(root.doc.startswith("https://") for root in registry.ROOTS))
    check("registry.no-codex-dotdir", not any(r.path.startswith(".codex/") for r in registry.ROOTS))
    check("registry.no-global-skills", not any("global_skills" in r.path for r in registry.ROOTS))
    kinds = {root.kind for root in registry.ROOTS}
    check("registry.kinds", kinds <= {"managed", "user", "project", "system", "extension"}, str(kinds))


def test_unicode_and_combining_paths(sandbox: Path) -> None:
    home = sandbox / "home"
    fixtures.write_skill(home / ".claude" / "skills", "日本語スキル")
    fixtures.write_skill(home / ".claude" / "skills", "café")
    fixtures.write_skill(home / ".agents" / "skills", "cafe\u0301")
    fixtures.write_skill(home / ".claude" / "skills", "emoji-\U0001f600")
    payload = discovery.collect(env_for(home))
    found = names(payload)
    check("unicode.cjk", "日本語スキル" in found, str(sorted(found)))
    check("unicode.precomposed", "café" in found)
    check("unicode.combining_kept_distinct", "cafe\u0301" in found)
    check("unicode.emoji", "emoji-\U0001f600" in found)
    for row in payload["items"]:
        check(f"unicode.no_control.{row['id']}", "\x00" not in row["name"] and "\n" not in row["name"])

def test_env_path_value_rules(sandbox: Path) -> None:
    check("envpath.keeps_edges", bounds.env_path_value({"X": "  spaced  "}, "X") == "  spaced  ")
    check("envpath.keeps_interior", bounds.env_path_value({"X": "a   b"}, "X") == "a   b")
    check("envpath.keeps_only_spaces", bounds.env_path_value({"X": "   "}, "X") == "   ")
    check("envpath.keeps_unicode", bounds.env_path_value({"X": "каталог/файл"}, "X") == "каталог/файл")
    check("envpath.unset_is_empty", bounds.env_path_value({}, "X") == "")
    check("envpath.empty_is_empty", bounds.env_path_value({"X": ""}, "X") == "")
    check("envpath.rejects_nul", bounds.env_path_value({"X": "a\x00b"}, "X") == "")
    check("envpath.rejects_oversize",
          bounds.env_path_value({"X": "x" * (bounds.MAX_ENV_PATH + 1)}, "X") == "")


def test_non_git_workspace_roots(sandbox: Path) -> None:
    original = registry.PROJECT_MARKERS
    registry.PROJECT_MARKERS = (".marker-absent-everywhere",)
    try:
        home = sandbox / "home"
        parent = sandbox / "родитель"
        workspace = parent / "рабочая-папка"
        wanted = {
            ".agents/skills": "общий-навык",
            ".opencode/skills": "опенкод-навык",
            ".pi/skills": "пи-навык",
            ".claude/skills": "клод-навык",
            ".github/skills": "гитхаб-навык",
        }
        for relative, name in wanted.items():
            fixtures.write_skill(workspace / relative, name)
        fixtures.write_skill(parent / ".agents" / "skills", "родительский-декой")
        fixtures.write_skill(parent / ".claude" / "skills", "второй-декой")

        payload = discovery.collect(env_for(home, workspace))
        found = names(payload)
        for name in wanted.values():
            check(f"nongit.finds.{name}", name in found, str(sorted(found)))
        check("nongit.parent_decoy_skipped", "родительский-декой" not in found, str(sorted(found)))
        check("nongit.second_decoy_skipped", "второй-декой" not in found)
        check("nongit.project_scope", all(row["scope"] in ("project", "user", "plugin", "extension",
                                                           "managed", "system")
                                          for row in payload["items"]))
        root, chain = discovery.project_chain(str(workspace))
        check("nongit.chain_is_anchor_only", chain == [str(workspace)], str(chain))
        check("nongit.root_is_anchor", root == str(workspace), root)
    finally:
        registry.PROJECT_MARKERS = original

def test_git_repository_ancestor_walk_is_preserved(sandbox: Path) -> None:
    home = sandbox / "home"
    outside = sandbox / "снаружи"
    repo = outside / "репозиторий"
    nested = repo / "пакеты" / "сервис"
    (repo / ".git").mkdir(parents=True)
    nested.mkdir(parents=True)
    fixtures.write_skill(repo / ".agents" / "skills", "корневой-навык")
    fixtures.write_skill(nested / ".agents" / "skills", "вложенный-навык")
    fixtures.write_skill(outside / ".agents" / "skills", "внешний-декой")

    payload = discovery.collect(env_for(home, nested))
    found = names(payload)
    check("git.finds_root_skill", "корневой-навык" in found, str(sorted(found)))
    check("git.finds_nested_skill", "вложенный-навык" in found)
    check("git.stops_at_repo_root", "внешний-декой" not in found, str(sorted(found)))
    root, chain = discovery.project_chain(str(nested))
    check("git.root_is_repo", root == str(repo), root)
    check("git.chain_stops_at_repo", chain[-1] == str(repo) and str(outside) not in chain, str(chain))

def test_empty_anchor_still_yields_no_project_roots(sandbox: Path) -> None:
    home = sandbox / "home"
    fixtures.write_skill(home / ".claude" / "skills", "личный")
    payload = discovery.collect(env_for(home))
    check("noanchor.user_only", "личный" in names(payload))
    check("noanchor.no_project_rows",
          all(row["scope"] != "project" for row in payload["items"]),
          str([row["scope"] for row in payload["items"]]))
    check("noanchor.chain_empty", discovery.project_chain("") == ("", []))

def row_id(env, name: str) -> str:
    row = row_for(discovery.collect(env), name)
    return row["id"] if row else ""

def agents_of(env, name: str) -> set[str]:
    row = row_for(discovery.collect(env), name)
    return set(row["agents"]) if row else set()

def snapshot(root: Path) -> set[tuple[str, str]]:
    found: set[tuple[str, str]] = set()
    for base, dirs, files in os.walk(root):
        for name in dirs + files:
            path = Path(base) / name
            kind = "link" if path.is_symlink() else ("dir" if path.is_dir() else "file")
            found.add((str(path), kind))
    return found

def test_apply_round_trip(sandbox: Path) -> None:
    home = sandbox / "home"
    fixtures.write_skill(home / ".claude" / "skills", "shared-skill")
    env = env_for(home)
    before = snapshot(sandbox)
    ident = row_id(env, "shared-skill")
    check("apply.list-is-read-only", snapshot(sandbox) == before)
    payload = apply.apply(env, ident, ["codex"], "on")
    result = payload["results"][0]
    link = home / ".agents" / "skills" / "shared-skill"
    check("apply.on-ok", payload["ok"] is True and result["ok"] is True, str(payload))
    check("apply.on-changed", result["changed"] is True)
    check("apply.on-symlink", link.is_symlink(), str(link))
    check("apply.on-target", os.path.realpath(link) == os.path.realpath(home / ".claude" / "skills" / "shared-skill"))
    check("apply.on-touched", result["touched"] == [str(link)], str(result["touched"]))
    check("apply.on-shared-note", "also applies to opencode, pi, copilot-cli" in result["message"],
          result["message"])
    check("apply.on-visible-after", {"codex", "pi"} <= agents_of(env, "shared-skill"))
    again = apply.apply(env, ident, ["codex"], "on")["results"][0]
    check("apply.on-idempotent", again["ok"] is True and again["changed"] is False and again["touched"] == [])
    off = apply.apply(env, ident, ["pi"], "off")["results"][0]
    check("apply.off-ok", off["ok"] is True and off["changed"] is True, str(off))
    check("apply.off-unlinked", not link.exists() and not link.is_symlink())
    check("apply.off-shared-note", "also applies to codex, opencode, copilot-cli" in off["message"],
          off["message"])
    check("apply.off-hidden-after", "codex" not in agents_of(env, "shared-skill"))
    check("apply.real-copy-kept", (home / ".claude" / "skills" / "shared-skill" / "SKILL.md").is_file())
    idle = apply.apply(env, ident, ["pi"], "off")["results"][0]
    check("apply.off-idempotent", idle["ok"] is True and idle["changed"] is False)

def test_apply_project_scope(sandbox: Path) -> None:
    home = sandbox / "home"
    repo = sandbox / "repo"
    nested = repo / "pkg"
    (repo / ".git").mkdir(parents=True)
    nested.mkdir()
    fixtures.write_skill(repo / ".claude" / "skills", "repo-skill")
    env = env_for(home, nested)
    ident = row_id(env, "repo-skill")
    result = apply.apply(env, ident, ["antigravity"], "on")["results"][0]
    link = repo / ".agents" / "skills" / "repo-skill"
    check("apply.project-root-used", link.is_symlink(), str(result))
    check("apply.project-note", "also applies to codex, opencode, pi, copilot-cli" in result["message"],
          result["message"])
    check("apply.project-not-under-home", not (home / ".agents").exists())
    none = apply.apply(env_for(home), ident, ["antigravity"], "on")["results"][0]
    check("apply.project-without-anchor-refused", none["ok"] is False, str(none))

def test_apply_all_agents(sandbox: Path) -> None:
    home = sandbox / "home"
    fixtures.write_skill(home / ".claude" / "skills", "everywhere")
    env = env_for(home)
    ident = row_id(env, "everywhere")
    payload = apply.apply(env, ident, ["all"], "on")
    agents = [row["agent"] for row in payload["results"]]
    check("apply.all-expands", agents == list(registry.agents()), str(agents))
    check("apply.all-ok", payload["ok"] is True, str(payload))
    check("apply.all-applied", agents_of(env, "everywhere") == set(registry.agents()),
          str(agents_of(env, "everywhere")))
    by_agent = {row["agent"]: row for row in payload["results"]}
    check("apply.all-claude-unchanged", by_agent["claude-code"]["changed"] is False)
    check("apply.all-shared-root-once",
          by_agent["codex"]["changed"] is True and by_agent["copilot-cli"]["changed"] is False,
          str(by_agent["copilot-cli"]))
    check("apply.all-pi-not-duplicated", by_agent["pi"]["changed"] is False, str(by_agent["pi"]))
    off = apply.apply(env, ident, ["codex", "pi", "antigravity"], "off")
    check("apply.multi-off-ok", off["ok"] is True, str(off))
    check("apply.multi-off-agents", "antigravity" not in agents_of(env, "everywhere"))

def test_apply_refusals(sandbox: Path) -> None:
    home = sandbox / "home"
    fixtures.write_skill(home / ".claude" / "skills", "guarded")
    env = env_for(home)
    ident = row_id(env, "guarded")
    real = apply.apply(env, ident, ["claude-code"], "off")["results"][0]
    check("apply.refuses-real-directory", real["ok"] is False and real["changed"] is False, str(real))
    check("apply.real-directory-kept", (home / ".claude" / "skills" / "guarded" / "SKILL.md").is_file())
    check("apply.refusal-names-path", str(home / ".claude" / "skills" / "guarded") in real["message"])
    fixtures.write_skill(home / ".agents" / "skills", "guarded", "an unrelated real skill")
    clash = apply.apply(env, ident, ["codex"], "on")["results"][0]
    check("apply.refuses-existing-non-symlink", clash["ok"] is False and clash["touched"] == [], str(clash))
    check("apply.existing-kept", not (home / ".agents" / "skills" / "guarded").is_symlink())
    elsewhere = sandbox / "elsewhere"
    fixtures.write_skill(elsewhere, "other")
    fixtures.link_skill(home / ".pi" / "agent" / "skills", "guarded", elsewhere / "other")
    foreign = apply.apply(env, ident, ["pi"], "on")["results"][0]
    check("apply.refuses-foreign-symlink", foreign["ok"] is False, str(foreign))
    check("apply.foreign-symlink-kept", os.readlink(home / ".pi" / "agent" / "skills" / "guarded") == str(elsewhere / "other"))
    unknown_agent = apply.apply(env, ident, ["nobody"], "on")
    check("apply.unknown-agent", unknown_agent["ok"] is False
          and unknown_agent["results"][0]["message"] == "unknown agent id nobody", str(unknown_agent))
    unknown_row = apply.apply(env, "0000000000000000", ["codex"], "on")
    check("apply.unknown-row", unknown_row["ok"] is False
          and unknown_row["results"][0]["message"] == "unknown row id", str(unknown_row))
    check("apply.name-escape-rejected", not apply.valid_name("../x") and not apply.valid_name("a/b")
          and not apply.valid_name("") and apply.valid_name("plain-name"))

def test_apply_scope_limits(sandbox: Path) -> None:
    home = sandbox / "home"
    prefix = sandbox / "prefix"
    fixtures.write_skill(prefix / "etc" / "codex" / "skills", "system-skill")
    env = env_for(home, prefix=prefix)
    system = apply.apply(env, row_id(env, "system-skill"), ["codex"], "on")["results"][0]
    check("apply.system-scope-refused", system["ok"] is False and "system" in system["message"], str(system))
    check("apply.scope-refusal-writes-nothing", not (home / ".agents").exists())

def test_apply_off_across_roots(sandbox: Path) -> None:
    home = sandbox / "home"
    real = fixtures.write_skill(home / ".pi" / "agent" / "skills", "multi").parent
    fixtures.link_skill(home / ".config" / "opencode" / "skills", "multi", real)
    fixtures.link_skill(home / ".agents" / "skills", "multi", real)
    env = env_for(home)
    ident = row_id(env, "multi")
    check("apply.multi-root-visible", "opencode" in agents_of(env, "multi"))
    result = apply.apply(env, ident, ["opencode"], "off")["results"][0]
    check("apply.multi-root-off-ok", result["ok"] is True and result["changed"] is True, str(result))
    check("apply.multi-root-both-unlinked", sorted(result["touched"]) == sorted([
        str(home / ".agents" / "skills" / "multi"), str(home / ".config" / "opencode" / "skills" / "multi")]),
        str(result["touched"]))
    check("apply.multi-root-real-kept", (real / "SKILL.md").is_file())
    check("apply.multi-root-note", "also applies to codex, pi, copilot-cli" in result["message"],
          result["message"])
    check("apply.multi-root-hidden", "opencode" not in agents_of(env, "multi"))
    fixtures.write_skill(home / ".claude" / "skills", "anchored")
    fixtures.link_skill(home / ".config" / "opencode" / "skills", "anchored",
                        home / ".claude" / "skills" / "anchored")
    blocked = apply.apply(env, row_id(env, "anchored"), ["opencode"], "off")["results"][0]
    check("apply.off-blocked-by-real-copy", blocked["ok"] is False and blocked["changed"] is False, str(blocked))
    check("apply.off-blocked-leaves-symlink", (home / ".config" / "opencode" / "skills" / "anchored").is_symlink())

def main() -> int:
    with tempfile.TemporaryDirectory() as raw:
        tmp = Path(raw)
        test_frontmatter()
        test_registry_shape()
        sandbox = tmp / "bounds"
        sandbox.mkdir()
        test_bounds(sandbox)
        metrics = tmp / "metrics"
        metrics.mkdir()
        test_artifact_metrics(metrics)
        for index, case in enumerate((
            test_scope_lanes, test_uppercase_only, test_documented_roots_only, test_shared_reader_attribution,
            test_symlink_dedupe, test_scope_not_merged, test_project_walk, test_exact_project,
            test_managed_and_system, test_reserved_synced, test_plugins_and_enable_state,
            test_codex_switches, test_output_bounds, test_stable_ids,
            test_hostile_inputs, test_empty_home,
            test_unicode_and_combining_paths,
            test_non_git_workspace_roots, test_git_repository_ancestor_walk_is_preserved,
            test_empty_anchor_still_yields_no_project_roots, test_env_path_value_rules,
            test_apply_round_trip, test_apply_project_scope, test_apply_all_agents,
            test_apply_refusals, test_apply_scope_limits, test_apply_off_across_roots,
        )):
            sandbox = tmp / f"case{index:02d}"
            sandbox.mkdir()
            case(sandbox)
    failures = [row for row in RESULTS if not row[1]]
    for name, ok, detail in RESULTS:
        if not ok:
            print(f"FAIL {name} {detail}")
    print(f"unit: {len(RESULTS) - len(failures)}/{len(RESULTS)} passed")
    return 1 if failures else 0

if __name__ == "__main__":
    raise SystemExit(main())
