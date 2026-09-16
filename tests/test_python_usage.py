import json
import os
import sqlite3
import stat
import subprocess
import tempfile
import unittest
from contextlib import closing
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DAY = timedelta(days=1)
EAST = "<+14>-14"


def iso(moment):
    return moment.astimezone(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def lines(*records):
    return "".join(json.dumps(record, separators=(",", ":")) + "\n" for record in records)


def envelope(kind, uuid, at, content, sidechain=False):
    return {"parentUuid": None, "isSidechain": sidechain, "userType": "external", "cwd": "/work/project",
            "sessionId": "0f1e2d3c", "version": "2.1.258", "gitBranch": "main", "type": kind, "uuid": uuid,
            "timestamp": iso(at), "message": {"role": kind, "content": content}}


def called(at, identity, name, sidechain=False, **arguments):
    part = {"type": "tool_use", "id": identity, "name": name, "input": arguments, "caller": {"type": "direct"}}
    return envelope("assistant", "a-" + identity, at, [part], sidechain)


def failed(at, identity):
    return envelope("user", "r-" + identity, at, [{"tool_use_id": identity, "type": "tool_result", "content": "boom", "is_error": True}])


def typed(at, uuid, *names, scheduled=None):
    content = "\n".join(f"<command-message>{name}</command-message>\n<command-name>/{name}</command-name>\n<command-args></command-args>"
                        for name in names)
    record = envelope("user", uuid, at, content)
    if scheduled:
        record["scheduledTaskId"] = scheduled
    return record


def opening():
    return {"type": "permission-mode", "permissionMode": "default", "sessionId": "0f1e2d3c"}


def session_meta(at):
    return {"timestamp": iso(at), "type": "session_meta",
            "payload": {"id": "c0de", "timestamp": iso(at), "cwd": "/work/project", "originator": "codex_cli_rs", "cli_version": "0.60.0"}}


def codex_call(at, identity, server, tool, status="completed"):
    item = {"type": "McpToolCall", "id": identity, "server": server, "tool": tool, "arguments": {"query": "x"},
            "readOnlyHint": True, "status": status, "result": {"content": []}, "duration": {"secs": 0, "nanos": 5}}
    return {"timestamp": iso(at), "type": "event_msg",
            "payload": {"type": "item_completed", "thread_id": "t", "turn_id": "u", "item": item}}


def mode(path):
    return stat.S_IMODE(path.stat().st_mode)


def uses(row):
    return {key: row[key] for key in ("uses", "usesAgent", "usesUser", "usesScheduled", "failed")}


class UsageHistory(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="fileblade-usage-")
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name)
        self.home = self.base / "home"
        self.project = self.base / "project"
        (self.project / ".git").mkdir(parents=True)
        self.claude = self.home / ".claude"
        self.transcripts = self.claude / "projects" / "-work-project"
        self.transcripts.mkdir(parents=True)
        self.store = self.base / "state" / "omarchy" / "fileblade" / "agent-usage.sqlite3"
        self.day = (datetime.now(timezone.utc) - 3 * DAY).replace(hour=11, minute=0, second=0, microsecond=0)
        self.env = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOME": str(self.home),
            "XDG_STATE_HOME": str(self.base / "state"),
            "XDG_CACHE_HOME": str(self.base / "cache"),
            "XDG_CONFIG_HOME": str(self.home / ".config"),
            "XDG_DATA_HOME": str(self.home / ".local" / "share"),
            "CLAUDE_CONFIG_DIR": str(self.claude),
            "CODEX_HOME": str(self.home / ".codex"),
            "TZ": "UTC0",
            "PYTHONDONTWRITEBYTECODE": "1",
        }

    def command(self, helper, *arguments, zone="UTC0"):
        return [str(ROOT / "python" / "bin" / f"agent-{helper}ctl"), *arguments], dict(self.env, TZ=zone)

    def helper(self, helper, *arguments, zone="UTC0"):
        command, env = self.command(helper, *arguments, zone=zone)
        process = subprocess.run(command, env=env, capture_output=True, text=True, timeout=60, check=False)
        self.assertEqual(process.returncode, 0, process.stderr + process.stdout)
        return json.loads(process.stdout)

    def skills(self, *arguments):
        return self.helper("skills", "list", "--json", "--project", str(self.project), *arguments)

    def skill_usage(self, zone="UTC0"):
        return self.helper("skills", "usage", "--json", "--project", str(self.project), zone=zone)

    def mcp(self):
        return self.helper("mcp", "list", "--json", "--project", str(self.project))

    def transcript(self, name, *records, mode="w"):
        path = self.transcripts / name
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open(mode) as handle:
            handle.write(lines(*records))
        return path

    def skill(self, directory, name):
        descriptor = directory / name / "SKILL.md"
        descriptor.parent.mkdir(parents=True)
        descriptor.write_text(f"---\nname: {name}\ndescription: fixture skill\n---\n")

    def plugin(self):
        install = self.claude / "plugins" / "cache" / "market" / "toolkit" / "1.0.0"
        self.skill(install / "skills", "review")
        (install / ".mcp.json").write_text(json.dumps({"mcpServers": {"docs": {"command": "docs-server"}}}))
        registry = {"version": 2, "plugins": {"toolkit@market": [{"scope": "user", "installPath": str(install), "version": "1.0.0"}]}}
        (self.claude / "plugins" / "installed_plugins.json").write_text(json.dumps(registry))

    def row(self, rows, name, **fields):
        found = [row for row in rows if row["name"] == name and all(row.get(key) == value for key, value in fields.items())]
        self.assertEqual(len(found), 1, [(row["name"], row.get("scope"), row.get("agent")) for row in rows])
        return found[0]

    def date(self, moment, zone=timezone.utc):
        return moment.astimezone(zone).date().isoformat()

    def test_skill_rows_separate_agent_typed_and_scheduled_uses(self):
        self.skill(self.claude / "skills", "alpha")
        self.skill(self.claude / "skills", "beta")
        self.skill(self.project / ".claude" / "skills", "gamma")
        self.plugin()
        first = called(self.day, "toolu_s1", "Skill", skill="alpha")
        self.transcript("s1.jsonl", opening(), first,
                        called(self.day, "toolu_s2", "Skill", skill="alpha", args="--fast"), failed(self.day, "toolu_s2"),
                        typed(self.day, "u1", "alpha"), typed(self.day, "u2", "alpha", scheduled="a36c8f4f"),
                        typed(self.day, "u3", "clear"), typed(self.day, "u4", "model"),
                        typed(self.day, "u5", "gamma", "toolkit:review"),
                        called(self.day, "toolu_s3", "Skill", skill="toolkit:review"))
        self.transcript("s1/subagents/workflows/wf_1/agent-a1.jsonl", called(self.day, "toolu_s4", "Skill", sidechain=True, skill="alpha"))
        copy = self.transcript("s2.jsonl", opening(), first, called(self.day, "toolu_s2", "Skill", skill="alpha", args="--fast"))
        os.utime(copy, (0, 0))
        document = self.skills()
        rows = document["items"]
        self.assertEqual(uses(self.row(rows, "alpha")), {"uses": 4, "usesAgent": 3, "usesUser": 1, "usesScheduled": 1, "failed": 1})
        self.assertEqual(uses(self.row(rows, "alpha")["metrics"]), uses(self.row(rows, "alpha")))
        self.assertEqual(self.row(rows, "beta")["uses"], 0)
        self.assertEqual(uses(self.row(rows, "gamma")), {"uses": 1, "usesAgent": 0, "usesUser": 1, "usesScheduled": 0, "failed": 0})
        self.assertEqual(uses(self.row(rows, "review", source="plugin:toolkit@market")),
                         {"uses": 2, "usesAgent": 1, "usesUser": 1, "usesScheduled": 0, "failed": 0})
        self.assertFalse({"clear", "model"} & {row["name"] for row in rows})
        self.assertEqual((document["usageTranscripts"], document["usageUnreadable"], document["usageIngestPending"]), (3, 0, False))
        history = self.skill_usage()
        self.assertEqual({key: history[key] for key in ("ok", "schemaVersion", "kind", "coverageStart", "ingestPending")},
                         {"ok": True, "schemaVersion": 1, "kind": "skill", "coverageStart": self.date(self.day), "ingestPending": False})
        self.assertEqual(history["until"], datetime.now(timezone.utc).date().isoformat())
        self.assertEqual(history["days"], [[self.date(self.day), 7, 4, 3, 1, 1]])

    def test_mcp_rows_follow_the_server_names_agents_record(self):
        self.plugin()
        servers = {name: {"command": "server"} for name in ("my.server", "twin.a", "twin_a", "quiet")}
        (self.home / ".claude.json").write_text(json.dumps({"mcpServers": servers}))
        (self.home / ".codex").mkdir(parents=True)
        (self.home / ".codex" / "config.toml").write_text(
            '[mcp_servers.docs]\ncommand = "docs"\n\n[mcp_servers.openaiDeveloperDocs]\nurl = "https://developers.openai.com/mcp"\n')
        later = self.day + timedelta(hours=1)
        self.transcript("s1.jsonl", opening(),
                        called(self.day, "toolu_m1", "mcp__my_server__lookup", query="a"),
                        called(later, "toolu_m2", "mcp__my_server__lookup", query="b"), failed(later, "toolu_m2"),
                        called(self.day, "toolu_m3", "ListMcpResourcesTool", server="my.server"),
                        called(self.day, "toolu_m4", "ReadMcpResourceTool", server="my.server", uri="probe://notes/alpha?version=2#top"),
                        typed(self.day, "p1", "mcp__my_server__greet"), typed(self.day, "p2", "mcp__my_server__greet", scheduled="b1"),
                        called(self.day, "toolu_m5", "mcp__twin_a__ping"),
                        called(self.day, "toolu_m6", "mcp__plugin_toolkit_docs__search", query="c"))
        rollout = self.home / ".codex" / "sessions" / "2026" / "09" / "14" / "rollout-2026-09-14T10-00-00-c0de.jsonl"
        rollout.parent.mkdir(parents=True)
        rollout.write_text(lines(session_meta(self.day), codex_call(self.day, "exec-1", "docs", "search"),
                                 codex_call(self.day, "exec-2", "openaiDeveloperDocs", "search_openai_docs"),
                                 codex_call(self.day, "exec-3", "openaiDeveloperDocs", "fetch_openai_doc", status="failed")))
        document = self.mcp()
        rows = document["definitions"]
        date = self.date(self.day)
        mine = self.row(rows, "my.server", agent="claude")
        self.assertEqual(uses(mine), {"uses": 5, "usesAgent": 4, "usesUser": 1, "usesScheduled": 1, "failed": 1})
        self.assertEqual(uses(mine["metrics"]), uses(mine))
        self.assertEqual(mine["observed"], [
            {"kind": "tool", "name": "lookup", "uses": 2, "failed": 1, "lastUsed": date},
            {"kind": "resource-list", "name": "", "uses": 1, "failed": 0, "lastUsed": date},
            {"kind": "prompt", "name": "greet", "uses": 1, "failed": 0, "lastUsed": date},
            {"kind": "resource", "name": "probe://notes/alpha", "uses": 1, "failed": 0, "lastUsed": date},
        ])
        for twin in ("twin.a", "twin_a"):
            row = self.row(rows, twin, agent="claude")
            self.assertEqual((row["uses"], row["usageAmbiguous"], row["observed"]), (0, True, []))
        self.assertEqual(document["usageAmbiguous"], 2)
        quiet = self.row(rows, "quiet", agent="claude")
        self.assertEqual((quiet["uses"], quiet["observed"], "usageAmbiguous" in quiet), (0, [], False))
        plugin = self.row(rows, "docs", agent="claude", scope="plugin")
        self.assertEqual((plugin["uses"], [entry["name"] for entry in plugin["observed"]]), (1, ["search"]))
        self.assertEqual(self.row(rows, "docs", agent="codex")["uses"], 1)
        openai = self.row(rows, "openaiDeveloperDocs", agent="codex")
        self.assertEqual(uses(openai), {"uses": 2, "usesAgent": 2, "usesUser": 0, "usesScheduled": 0, "failed": 1})
        self.assertEqual((document["usageTranscripts"], document["usageIngestPending"]), (2, False))
        history = self.helper("mcp", "usage", "--json")
        self.assertEqual((history["kind"], history["coverageStart"], history["days"]), ("mcp", date, [[date, 10, 9, 1, 1, 2]]))

    def test_an_appended_transcript_is_read_from_where_the_last_read_stopped(self):
        self.skill(self.claude / "skills", "alpha")
        path = self.transcript("s1.jsonl", opening(), called(self.day, "toolu_a1", "Skill", skill="alpha"))
        self.assertEqual(self.row(self.skills()["items"], "alpha")["usesAgent"], 1)
        rewritten = path.read_bytes().replace(b"toolu_a1", b"toolu_b1")
        with path.open("r+b") as handle:
            handle.write(rewritten)
        self.transcript("s1.jsonl", called(self.day, "toolu_a2", "Skill", skill="alpha"), mode="a")
        self.assertEqual(self.row(self.skills()["items"], "alpha")["usesAgent"], 2)

    def test_a_replaced_transcript_is_not_counted_twice(self):
        self.skill(self.claude / "skills", "alpha")
        records = [called(self.day, f"toolu_r{index}", "Skill", skill="alpha") for index in range(3)]
        path = self.transcript("s1.jsonl", *records[:2])
        self.assertEqual(self.row(self.skills()["items"], "alpha")["usesAgent"], 2)
        replacement = self.transcript("s1.jsonl.new", *records)
        os.replace(replacement, path)
        self.assertEqual(self.row(self.skills()["items"], "alpha")["usesAgent"], 3)

    def test_a_half_written_last_line_waits_for_its_newline(self):
        self.skill(self.claude / "skills", "alpha")
        second = lines(called(self.day, "toolu_h2", "Skill", skill="alpha"))
        path = self.transcript("s1.jsonl", called(self.day, "toolu_h1", "Skill", skill="alpha"))
        with path.open("a") as handle:
            handle.write(second[:40])
        self.assertEqual(self.row(self.skills()["items"], "alpha")["usesAgent"], 1)
        with path.open("a") as handle:
            handle.write(second[40:])
        self.assertEqual(self.row(self.skills()["items"], "alpha")["usesAgent"], 2)

    def test_a_deleted_transcript_keeps_its_uses_and_the_coverage_start(self):
        self.skill(self.claude / "skills", "alpha")
        oldest = self.day - 10 * DAY
        old = self.transcript("old.jsonl", opening(), called(oldest, "toolu_o1", "Skill", skill="alpha"))
        self.transcript("new.jsonl", opening(), called(self.day, "toolu_n1", "Skill", skill="alpha"))
        document = self.skills()
        self.assertEqual((self.row(document["items"], "alpha")["uses"], document["usageTranscripts"]), (2, 2))
        self.assertEqual(self.skill_usage()["coverageStart"], self.date(oldest))
        old.unlink()
        self.transcript("new.jsonl", called(self.day, "toolu_n2", "Skill", skill="alpha"), mode="a")
        document = self.skills()
        self.assertEqual((self.row(document["items"], "alpha")["uses"], document["usageTranscripts"]), (3, 1))
        history = self.skill_usage()
        self.assertEqual((history["coverageStart"], [day[0] for day in history["days"]]), (self.date(oldest), [self.date(oldest), self.date(self.day)]))

    def test_a_typed_command_counts_whichever_lane_reads_it_first(self):
        self.skill(self.claude / "skills", "pdf")
        self.skill(self.project / ".claude" / "skills", "gamma")
        self.transcript("s1.jsonl", typed(self.day, "u1", "pdf"))
        self.mcp()
        self.transcript("s1.jsonl", typed(self.day, "u2", "pdf"), mode="a")
        project = self.skills("--scope", "project")
        self.assertEqual([row["name"] for row in project["items"]], ["gamma"])
        self.assertEqual(self.row(self.skills("--scope", "user")["items"], "pdf")["usesUser"], 2)

    def test_days_are_local_dates(self):
        self.skill(self.claude / "skills", "alpha")
        self.transcript("s1.jsonl", called(self.day, "toolu_t1", "Skill", skill="alpha"))
        utc = self.skill_usage()
        east = self.skill_usage(zone=EAST)
        zone = timezone(timedelta(hours=14))
        self.assertNotEqual(self.date(self.day), self.date(self.day, zone))
        self.assertEqual(utc["days"], [[self.date(self.day), 1, 1, 0, 0, 0]])
        self.assertEqual(east["days"], [[self.date(self.day, zone), 1, 1, 0, 0, 0]])
        self.assertEqual((east["coverageStart"], east["until"]), (self.date(self.day, zone), datetime.now(zone).date().isoformat()))

    def test_concurrent_helpers_ingest_once_and_agree(self):
        self.assertEqual(self.helper("mcp", "usage", "--json")["days"], [])
        with closing(sqlite3.connect(self.store)) as connection:
            connection.executescript("""
                CREATE TABLE ingest_log (path TEXT NOT NULL);
                CREATE TRIGGER source_inserted AFTER INSERT ON source BEGIN INSERT INTO ingest_log VALUES (NEW.path); END;
                CREATE TRIGGER source_updated AFTER UPDATE ON source BEGIN INSERT INTO ingest_log VALUES (NEW.path); END;
            """)
        for session in range(24):
            self.transcript(f"s{session}.jsonl", opening(), *(called(self.day, f"toolu_{session}_{call}", f"mcp__server{call}__tool")
                                                              for call in range(40)))
        command, env = self.command("mcp", "usage", "--json")
        processes = [subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE) for _ in range(4)]
        answers = [process.communicate(timeout=60) for process in processes]
        self.assertEqual([process.returncode for process in processes], [0, 0, 0, 0], [error for _, error in answers])
        self.assertEqual(len({output for output, _ in answers}), 1)
        history = json.loads(answers[0][0])
        self.assertEqual((history["ingestPending"], history["days"]), (False, [[self.date(self.day), 960, 960, 0, 0, 0]]))
        with closing(sqlite3.connect(self.store)) as connection:
            writes = connection.execute("SELECT count(DISTINCT path), count(*) FROM ingest_log").fetchone()
        self.assertEqual(writes, (24, 24))

    def test_forget_removes_history_before_a_day_or_all_of_it(self):
        self.skill(self.claude / "skills", "alpha")
        early = self.day - 5 * DAY
        self.transcript("s1.jsonl", called(early, "toolu_f1", "Skill", skill="alpha"), called(self.day, "toolu_f2", "Skill", skill="alpha"))
        self.assertEqual(self.row(self.skills()["items"], "alpha")["uses"], 2)
        cutoff = self.date(self.day - DAY)
        self.assertEqual(self.helper("mcp", "usage-forget", "--json", "--before", cutoff), {"ok": True, "schemaVersion": 1, "removed": 1})
        history = self.skill_usage()
        self.assertEqual((history["coverageStart"], history["days"]), (cutoff, [[self.date(self.day), 1, 1, 0, 0, 0]]))
        self.assertEqual(self.row(self.skills()["items"], "alpha")["uses"], 1)
        self.assertEqual(self.helper("mcp", "usage-forget", "--json"), {"ok": True, "schemaVersion": 1, "removed": 1})
        self.assertEqual(self.row(self.skills()["items"], "alpha")["uses"], 0)
        history = self.skill_usage()
        self.assertEqual((history["coverageStart"], history["days"]), (None, []))
        with closing(sqlite3.connect(self.store)) as connection:
            self.assertEqual(connection.execute("SELECT count(*) FROM event UNION ALL SELECT count(*) FROM project").fetchall(), [(0,), (0,)])

    def test_the_store_is_private_and_the_old_cache_is_removed(self):
        cache = self.base / "cache" / "omarchy" / "fileblade"
        cache.mkdir(parents=True)
        for name in ("agent-usage.json", "agent-usage.tmp"):
            (cache / name).write_text('{"schema": 1, "files": {}}')
        self.skills()
        self.assertEqual(sorted(path.name for path in cache.iterdir()), [])
        self.assertEqual(mode(self.store.parent), 0o700)
        self.assertEqual((mode(self.store), mode(self.store.with_name("agent-usage.sqlite3.lock"))), (0o600, 0o600))
        self.assertFalse((self.home / ".local" / "state").exists())

    @unittest.skipIf(os.geteuid() == 0, "root reads every file")
    def test_an_unreadable_transcript_is_reported(self):
        self.skill(self.claude / "skills", "alpha")
        blocked = self.transcript("s1.jsonl", called(self.day, "toolu_u1", "Skill", skill="alpha"))
        blocked.chmod(0)
        self.addCleanup(blocked.chmod, 0o600)
        document = self.skills()
        self.assertEqual((document["usageUnreadable"], self.row(document["items"], "alpha")["uses"]), (1, 0))


if __name__ == "__main__":
    unittest.main()
