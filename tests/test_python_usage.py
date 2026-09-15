import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))
import agent_usage


def line(record):
    return json.dumps(record) + "\n"


def skill_call(name, identity="t1"):
    return line({"type": "assistant", "message": {"content": [
        {"type": "tool_use", "id": identity, "name": "Skill", "input": {"skill": name}}]}})


def mcp_call(server, tool="do", identity="t2"):
    return line({"type": "assistant", "message": {"content": [
        {"type": "tool_use", "id": identity, "name": f"mcp__{server}__{tool}", "input": {}}]}})


def failure(identity):
    return line({"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": identity, "is_error": True, "content": "boom"}]}})


def typed(name, scheduled=None):
    record = {"type": "user", "userType": "external",
              "message": {"content": f"<command-name>/{name}</command-name>"}}
    if scheduled:
        record["scheduledTaskId"] = scheduled
    return line(record)


class UsageTests(unittest.TestCase):
    def collect(self, root, known={"omarchy", "pdf"}, cache=None, use_cache=False):
        return agent_usage.collect(known=known, roots=[root],
                                   cache=cache, use_cache=use_cache)

    def test_the_agent_and_the_person_are_counted_apart(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "a.jsonl").write_text(skill_call("omarchy") + typed("pdf") + skill_call("pdf", "t9"))
            result = self.collect(root)
            self.assertEqual(result["skills"]["omarchy"],
                             {"uses": 1, "usesAgent": 1, "usesUser": 0, "usesScheduled": 0, "failed": 0})
            self.assertEqual(result["skills"]["pdf"]["usesUser"], 1)
            self.assertEqual(result["skills"]["pdf"]["usesAgent"], 1)
            self.assertEqual(result["skills"]["pdf"]["uses"], 2)

    def test_a_timer_firing_is_not_a_person(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "a.jsonl").write_text(typed("pdf", scheduled="a36c8f4f") + typed("pdf"))
            counts = self.collect(root)["skills"]["pdf"]
            self.assertEqual((counts["usesUser"], counts["usesScheduled"]), (1, 1))
            self.assertEqual(counts["uses"], 1)

    def test_a_command_that_is_not_a_skill_is_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "a.jsonl").write_text(typed("goal") + typed("clear") + typed("pdf"))
            self.assertEqual(set(self.collect(root)["skills"]), {"pdf"})

    def test_a_failed_call_still_counts_as_an_attempt_and_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "a.jsonl").write_text(skill_call("omarchy", "x1") + failure("x1")
                                          + mcp_call("tabular-editor", identity="m1") + failure("m1"))
            result = self.collect(root)
            self.assertEqual(result["skills"]["omarchy"]["failed"], 1)
            self.assertEqual(result["skills"]["omarchy"]["uses"], 1)
            self.assertEqual(result["servers"]["tabular-editor"]["failed"], 1)

    def test_subagent_transcripts_in_subdirectories_are_read(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            nested = root / "session" / "subagents" / "workflows"
            nested.mkdir(parents=True)
            (nested / "journal.jsonl").write_text(skill_call("omarchy"))
            self.assertEqual(self.collect(root)["skills"]["omarchy"]["usesAgent"], 1)

    def test_a_growing_transcript_reads_only_what_was_appended(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cache = root / "cache.json"
            transcript = root / "a.jsonl"
            transcript.write_text(skill_call("omarchy"))
            first = agent_usage.collect(known={"omarchy"}, roots=[root], cache=cache, use_cache=True)
            self.assertEqual(first["skills"]["omarchy"]["usesAgent"], 1)
            offset = agent_usage.load_cache(cache)[str(transcript)].offset
            self.assertEqual(offset, transcript.stat().st_size)
            with transcript.open("a") as handle:
                handle.write(skill_call("omarchy", "t5"))
            second = agent_usage.collect(known={"omarchy"}, roots=[root], cache=cache, use_cache=True)
            self.assertEqual(second["skills"]["omarchy"]["usesAgent"], 2)

    def test_a_replaced_transcript_is_counted_from_the_start(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cache = root / "cache.json"
            transcript = root / "a.jsonl"
            transcript.write_text(skill_call("omarchy") + skill_call("omarchy", "t2"))
            agent_usage.collect(known={"omarchy"}, roots=[root], cache=cache, use_cache=True)
            transcript.unlink()
            transcript.write_text(skill_call("omarchy"))
            again = agent_usage.collect(known={"omarchy"}, roots=[root], cache=cache, use_cache=True)
            self.assertEqual(again["skills"]["omarchy"]["usesAgent"], 1)

    def test_a_half_written_last_line_waits_for_its_newline(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cache = root / "cache.json"
            transcript = root / "a.jsonl"
            transcript.write_text(skill_call("omarchy") + skill_call("omarchy", "t2").rstrip("\n"))
            first = agent_usage.collect(known={"omarchy"}, roots=[root], cache=cache, use_cache=True)
            self.assertEqual(first["skills"]["omarchy"]["usesAgent"], 1)
            with transcript.open("a") as handle:
                handle.write("\n")
            second = agent_usage.collect(known={"omarchy"}, roots=[root], cache=cache, use_cache=True)
            self.assertEqual(second["skills"]["omarchy"]["usesAgent"], 2)

    def test_an_unreadable_transcript_is_coverage_loss_not_zero(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            blocked = root / "a.jsonl"
            blocked.write_text(skill_call("omarchy"))
            os.chmod(blocked, 0)
            try:
                result = self.collect(root)
                self.assertEqual(result["unreadable"], 1)
                self.assertTrue(result["errors"])
            finally:
                os.chmod(blocked, 0o600)

    def test_mcp_servers_are_named_by_their_middle_segment(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "a.jsonl").write_text(mcp_call("claude-in-chrome", "navigate")
                                          + mcp_call("plugin_fabric-cli_microsoft-learn", "search", "m2"))
            servers = self.collect(root)["servers"]
            self.assertEqual(servers["claude-in-chrome"]["uses"], 1)
            self.assertIn("plugin_fabric-cli_microsoft-learn", servers)


if __name__ == "__main__":
    unittest.main()


class ServerNameTests(unittest.TestCase):
    def test_a_plugin_server_offers_its_bare_name_too(self):
        self.assertEqual(agent_usage.server_aliases("plugin_fabric-cli_microsoft-learn"),
                         ["plugin_fabric-cli_microsoft-learn", "fabric-cli_microsoft-learn", "microsoft-learn"])

    def test_an_ordinary_server_offers_only_itself(self):
        self.assertEqual(agent_usage.server_aliases("tabular-editor"), ["tabular-editor"])
