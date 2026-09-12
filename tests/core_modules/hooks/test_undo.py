from copy import deepcopy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_hooks import apply, discovery


class HookUndo(unittest.TestCase):
    def setUp(self):
        self.sandbox = tempfile.TemporaryDirectory(prefix="fileblade-hooks-undo-")
        self.addCleanup(self.sandbox.cleanup)
        self.root = Path(self.sandbox.name)
        self.home = self.root / "home"
        self.project = self.root / "project"
        self.home.mkdir()
        self.project.mkdir()
        self.source = self.project / ".claude" / "settings.json"
        self.source.parent.mkdir()
        self.agent = "claude-code"
        self.event = "PreToolUse"
        self.container = "hooks"
        self.env = {"HOME": str(self.home)}

    def write(self, definitions):
        self.original = {"other": {"keep": True}, self.container: {self.event: definitions}}
        if self.agent == "copilot-cli":
            self.original["version"] = 1
        self.source.write_text(json.dumps(self.original))

    def rows(self):
        inventory = discovery.collect(str(self.project), str(self.home), self.env,
                                      str(self.root / "etc"), os.getuid(), exact=True)
        return [row for row in inventory["items"]
                if row["agent"] == self.agent and str(row["source"]["path"]) == str(self.source)]

    def remove(self, row):
        return apply.remove(str(self.project), row["id"], str(self.home), self.env,
                            str(self.root / "etc"), os.getuid(), exact=True)

    def mint(self, payload):
        context = {"project": str(self.project), "home": str(self.home),
                   "etcRoot": str(self.root / "etc"), "policyOwnerUid": str(os.getuid())}
        return apply.recovery_store(str(self.home), self.env).write(payload, "fixture", context)

    def current(self):
        return json.loads(self.source.read_text())

    def assert_round_trip(self, index=0):
        removed = self.remove(self.rows()[index])
        self.assertTrue(removed["ok"], removed)
        restored = apply.restore("", json.dumps(removed["payload"]), str(self.home), self.env)
        self.assertTrue(restored["ok"], restored)
        self.assertEqual(self.current(), self.original)
        again = apply.restore("", json.dumps(removed["payload"]), str(self.home), self.env)
        self.assertTrue(again["ok"], again)
        self.assertFalse(again["results"][0]["changed"])
        return removed

    def test_exact_fields_timeout_group_and_position(self):
        self.write([
            {"hooks": [{"type": "command", "command": "printf before"}]},
            {"matcher": "Bash", "groupExtension": {"keep": [1, False]}, "hooks": [
                {"type": "command", "command": "printf first"},
                {"type": "command", "command": "printf selected", "timeout": 0.5,
                 "entryExtension": {"keep": True}, "if": "test fixture"},
                {"type": "command", "command": "printf last"}]},
            {"hooks": [{"type": "command", "command": "printf after"}]}])
        self.assert_round_trip(2)

    def test_prepared_removal_preserves_the_source_and_compares_the_complete_record(self):
        self.write([{"hooks": [{"type": "command", "command": "printf first"},
                               {"type": "command", "command": "printf selected"}]}])
        row = self.rows()[1]
        before = self.source.read_bytes()
        args = (str(self.project), row["id"], str(self.home), self.env, str(self.root / "etc"), os.getuid())
        prepared = apply.remove(*args, exact=True, prepare=True)
        self.assertTrue(prepared["ok"], prepared)
        self.assertEqual(self.source.read_bytes(), before)
        tampered = deepcopy(prepared["payload"])
        tampered["source"] = str(self.root / "elsewhere")
        refused = apply.remove(*args, exact=True, expected_payload=tampered)
        self.assertFalse(refused["ok"], refused)
        self.assertEqual(self.source.read_bytes(), before)
        committed = apply.remove(*args, exact=True, expected_payload=prepared["payload"])
        self.assertTrue(committed["ok"], committed)
        self.assertTrue(apply.restore("", json.dumps(prepared["payload"]), str(self.home), self.env)["ok"])
        self.assertEqual(self.current(), self.original)

    def test_last_entry_restores_all_group_metadata(self):
        self.write([{"matcher": "Bash", "enabled": False, "extra": "keep", "hooks": [
            {"type": "command", "command": "printf selected", "timeout": 0.5}]}])
        self.assert_round_trip()

    def test_duplicate_commands_remove_only_selected_record(self):
        entry = {"type": "command", "command": "printf duplicate"}
        self.write([{"matcher": "Bash", "hooks": [entry, deepcopy(entry)]},
                    {"matcher": "Read", "hooks": [deepcopy(entry)]}])
        removed = self.remove(self.rows()[1])
        self.assertTrue(removed["ok"], removed)
        expected = deepcopy(self.original)
        expected["hooks"][self.event][0]["hooks"].pop(1)
        self.assertEqual(self.current(), expected)
        self.assertTrue(apply.restore("", json.dumps(removed["payload"]), str(self.home), self.env)["ok"])
        self.assertEqual(self.current(), self.original)

    def test_group_larger_than_one_hundred_has_unique_address(self):
        self.write([{"hooks": [{"type": "command", "command": "printf first-" + str(i)}
                                for i in range(101)]},
                    {"hooks": [{"type": "command", "command": "printf second"}]}])
        rows = self.rows()
        self.assertEqual(len(rows), 102)
        self.assertEqual(len({row["id"] for row in rows}), 102)
        self.assertEqual(apply.source_entry(rows[100], self.original)["command"], "printf first-100")
        self.assert_round_trip(100)

    def test_stale_row_cannot_remove_or_copy_replacement(self):
        self.write([{"hooks": [{"type": "command", "command": "printf same", "timeout": 1}]}])
        old = self.rows()[0]
        replacement = deepcopy(self.original)
        replacement["hooks"][self.event][0]["hooks"][0]["timeout"] = 2
        self.source.write_text(json.dumps(replacement))
        self.assertNotEqual(old["id"], self.rows()[0]["id"])
        self.assertFalse(self.remove(old)["ok"])
        copied = apply.apply(str(self.project), old["id"], ["codex"], "on", str(self.home),
                             self.env, str(self.root / "etc"), os.getuid(), exact=True)
        self.assertFalse(copied["ok"], copied)
        self.assertFalse((self.home / ".codex" / "hooks.json").exists())
        self.assertEqual(self.current(), replacement)

    def test_restore_preserves_other_events_and_settings(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        removed = self.remove(self.rows()[0])
        self.assertTrue(removed["ok"], removed)
        changed = self.current()
        changed["other"]["added"] = 42
        changed["hooks"]["Stop"] = [{"hooks": [{"command": "printf later"}]}]
        self.source.write_text(json.dumps(changed))
        restored = apply.restore("", json.dumps(removed["payload"]), str(self.home), self.env)
        self.assertTrue(restored["ok"], restored)
        changed["hooks"][self.event] = self.original["hooks"][self.event]
        self.assertEqual(self.current(), changed)

    def test_restore_conflict_keeps_current_event_untouched(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        removed = self.remove(self.rows()[0])
        self.assertTrue(removed["ok"], removed)
        changed = self.current()
        changed["hooks"][self.event] = [{"hooks": [{"command": "printf later"}]}]
        self.source.write_text(json.dumps(changed))
        before = self.source.read_bytes()
        restored = apply.restore("", json.dumps(removed["payload"]), str(self.home), self.env)
        self.assertFalse(restored["ok"], restored)
        self.assertIn("changed", restored["message"])
        self.assertEqual(self.source.read_bytes(), before)

    def test_flat_copilot_record_round_trip(self):
        self.source = self.project / ".github" / "hooks" / "fixture.json"
        self.source.parent.mkdir(parents=True)
        self.agent, self.event = "copilot-cli", "preToolUse"
        self.write([{"type": "command", "bash": "printf selected", "timeoutSec": 0.25,
                     "powershell": "Write-Host fixture", "extra": {"keep": True}}])
        self.assert_round_trip()

    def test_antigravity_named_group_round_trip(self):
        self.source = self.project / ".agents" / "hooks.json"
        self.source.parent.mkdir()
        self.agent, self.container = "antigravity", "original group"
        self.write([{"matcher": "Bash", "extra": "group", "hooks": [
            {"type": "command", "command": "printf selected", "extra": "entry"}]}])
        self.original[self.container]["enabled"] = False
        self.source.write_text(json.dumps(self.original))
        self.assert_round_trip()

    def test_retargeted_symlink_cannot_restore_into_another_file(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        real = self.root / "real.json"
        self.source.rename(real)
        self.source.symlink_to(real)
        removed = self.remove(self.rows()[0])
        self.assertTrue(removed["ok"], removed)
        other = self.root / "other.json"
        other.write_bytes(real.read_bytes())
        self.source.unlink()
        self.source.symlink_to(other)
        restored = apply.restore("", json.dumps(removed["payload"]), str(self.home), self.env)
        self.assertFalse(restored["ok"], restored)
        self.assertEqual(other.read_bytes(), real.read_bytes())

    def test_invalid_restore_records_are_refused_without_writes(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        removed = self.remove(self.rows()[0])
        self.assertTrue(removed["ok"], removed)
        before = self.source.read_bytes()
        for key, value in (("index", -1), ("index", True), ("entry", []), ("grouped", "true"),
                           ("fields", {"hooks": []}), ("before", "0" * 64), ("format", 3),
                           ("event", []), ("source", {}), ("group", {})):
            with self.subTest(key=key, value=value):
                payload = dict(removed["payload"], **{key: value})
                self.assertFalse(apply.restore(self.mint(payload), json.dumps(payload), str(self.home), self.env)["ok"])
                self.assertEqual(self.source.read_bytes(), before)

    def test_a_project_source_restores_without_project_arguments(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        removed = self.remove(self.rows()[0])
        self.assertTrue(removed["ok"], removed)
        restored = apply.restore(removed["recordId"], json.dumps(removed["payload"]), str(self.home), self.env)
        self.assertTrue(restored["ok"], restored)
        self.assertEqual(self.current(), self.original)

    def test_copilot_settings_and_plugin_sources_round_trip(self):
        self.agent = "copilot-cli"
        for source in (self.home / ".copilot" / "settings.json",
                       self.project / ".github" / "copilot" / "settings.json",
                       self.home / ".copilot" / "installed-plugins" / "market" / "sample" / "hooks.json"):
            with self.subTest(source=str(source)):
                self.source = source
                self.source.parent.mkdir(parents=True, exist_ok=True)
                if "installed-plugins" in str(self.source):
                    (self.source.parent / "plugin.json").write_text(json.dumps({"name": "sample"}))
                self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
                rows = self.rows()
                self.assertTrue(rows, f"no row for {source}")
                removed = self.remove(rows[0])
                self.assertTrue(removed["ok"], removed)
                restored = apply.restore(removed["recordId"], json.dumps(removed["payload"]),
                                         str(self.home), self.env)
                self.assertTrue(restored["ok"], restored)
                self.assertEqual(self.current(), self.original)

    def test_the_store_refuses_a_new_removal_instead_of_evicting_undo_records(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        row = self.rows()[0]
        store = apply.recovery_store(str(self.home), self.env)
        context = {"project": str(self.project), "home": str(self.home),
                   "etcRoot": str(self.root / "etc"), "policyOwnerUid": str(os.getuid())}
        for index in range(64):
            store.write({"format": 2, "filler": index}, "fixture", context)
        before = self.source.read_bytes()
        refused = self.remove(row)
        self.assertFalse(refused["ok"], refused)
        self.assertIn("undo store is full", refused["message"])
        self.assertEqual(self.source.read_bytes(), before)

    def test_a_restored_record_stops_holding_a_place_in_the_store(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        removed = self.remove(self.rows()[0])
        self.assertTrue(removed["ok"], removed)
        store = apply.recovery_store(str(self.home), self.env)
        self.assertEqual(store.live_records(), 1)
        restored = apply.restore(removed["recordId"], json.dumps(removed["payload"]), str(self.home), self.env)
        self.assertTrue(restored["ok"], restored)
        self.assertEqual(store.live_records(), 0)
        again = apply.restore(removed["recordId"], json.dumps(removed["payload"]), str(self.home), self.env)
        self.assertTrue(again["ok"], again)
        self.assertFalse(again["results"][0]["changed"])

    def test_forged_payload_cannot_write_outside_the_known_hook_files(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        removed = self.remove(self.rows()[0])
        self.assertTrue(removed["ok"], removed)
        outsider = self.home / "unrelated.json"
        outsider.write_text(json.dumps({"keep": True}))
        before = outsider.read_bytes()
        forged = dict(removed["payload"], source=str(outsider), target=str(outsider))
        self.assertFalse(apply.restore("", json.dumps(forged), str(self.home), self.env)["ok"])
        self.assertEqual(outsider.read_bytes(), before)
        stored = apply.restore(self.mint(forged), json.dumps(forged), str(self.home), self.env)
        self.assertFalse(stored["ok"], stored)
        self.assertEqual(outsider.read_bytes(), before)

    def test_restore_needs_a_prepared_record_and_reports_its_identifier(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        removed = self.remove(self.rows()[0])
        self.assertTrue(removed["ok"], removed)
        self.assertEqual(len(removed["recordId"]), 32)
        restored = apply.restore(removed["recordId"], json.dumps(removed["payload"]), str(self.home), self.env)
        self.assertTrue(restored["ok"], restored)
        removed_again = self.remove(self.rows()[0])
        self.assertTrue(removed_again["ok"], removed_again)
        apply.recovery_store(str(self.home), self.env).discard(removed_again["recordId"])
        apply.recovery_store(str(self.home), self.env).discard(removed["recordId"])
        after = self.source.read_bytes()
        refused = apply.restore(removed_again["recordId"], json.dumps(removed_again["payload"]), str(self.home), self.env)
        self.assertFalse(refused["ok"], refused)
        self.assertEqual(self.source.read_bytes(), after)

    def test_unencodable_other_fields_refuse_before_temporary_creation(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        broken = dict(self.original, unrelated="\udcff")
        self.source.write_text(json.dumps(broken))
        before = self.source.read_bytes()
        removed = self.remove(self.rows()[0])
        self.assertFalse(removed["ok"], removed)
        self.assertEqual(self.source.read_bytes(), before)
        self.assertEqual(list(self.source.parent.iterdir()), [self.source])

    def test_refused_native_publish_preserves_the_original(self):
        self.write([{"hooks": [{"type": "command", "command": "printf selected"}]}])
        before = self.source.read_bytes()
        row = self.rows()[0]
        with patch.object(apply.Snapshot, "write", side_effect=OSError("fixture publish failure")):
            removed = self.remove(row)
        self.assertFalse(removed["ok"], removed)
        self.assertEqual(self.source.read_bytes(), before)
        self.assertEqual(list(self.source.parent.iterdir()), [self.source])

    def test_duplicate_json_members_are_not_silently_rewritten(self):
        self.source.write_text('{"other":1,"other":2,"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"printf fixture"}]}]}}')
        row = self.rows()[0]
        before = self.source.read_bytes()
        self.assertFalse(self.remove(row)["ok"])
        self.assertEqual(self.source.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
