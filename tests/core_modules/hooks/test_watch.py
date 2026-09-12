import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_hooks import discovery
from fileblade_inventory import WatchPlan
from fileblade_paths import path_text

ROOT = Path(__file__).resolve().parents[3]


class HooksWatchTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.home, self.project = self.base / "home", self.base / "project"
        self.home.mkdir(); self.project.mkdir()

    def scan(self):
        with WatchPlan() as plan:
            document = plan.finish(discovery.collect(str(self.project), str(self.home),
                                                   environ={}, etc_root=str(self.base / "etc"), exact=True))
        return document, plan.paths

    def test_missing_sources_and_nested_code_directories_are_watched(self):
        _, paths = self.scan()
        self.assertIn(self.project, paths)
        self.assertIn(self.home, paths)
        source = self.project / ".pi" / "extensions" / "nested"
        source.mkdir(parents=True)
        (source / "hook.ts").write_text("SHOULD_NOT_BE_EXECUTED")
        document, paths = self.scan()
        self.assertIn(source, paths)
        self.assertIn(source.parent, paths)
        self.assertNotIn("SHOULD_NOT_BE_EXECUTED", json.dumps(document))
        self.assertTrue(any(row["agent"] == "pi" for row in document["items"]))

    def test_symlink_target_and_logical_parent_are_both_watched(self):
        source = self.project / ".claude" / "settings.json"
        target = self.base / "vault" / "settings.json"
        source.parent.mkdir(); target.parent.mkdir()
        target.write_text('{"hooks":{"Stop":[{"command":"private-command"}]}}')
        source.symlink_to(target)
        document, paths = self.scan()
        self.assertIn(source.parent, paths)
        self.assertIn(target.parent, paths)
        self.assertTrue(document["items"])
        self.assertNotIn("private-command", json.dumps(document))

    def test_cli_watch_paths_are_opt_in_and_native_byte_faithful(self):
        self.project = self.base / os.fsdecode(b"project-\xff")
        self.project.mkdir()
        arguments = [str(ROOT / "python/bin/agent-hooksctl"), "list", "--project", path_text(str(self.project)),
                     "--home", str(self.home), "--exact", "--json"]
        plain = json.loads(subprocess.check_output(arguments))
        self.assertNotIn("watchPaths", plain)
        watched = json.loads(subprocess.check_output(arguments + ["--watch"]))
        self.assertIn(path_text(str(self.project)), watched["watchPaths"])
        self.assertNotIn("\udcff", json.dumps(watched, ensure_ascii=False))
        self.assertLessEqual(len(watched["watchPaths"]), 512)

    def test_declared_helper_runs_through_native_backend(self):
        from fileblade_paths import __file__ as shared_path
        binary = Path(os.environ.get("FILEBLADE_BINARY", str(Path(shared_path).resolve().parents[1] / "target/release/fileblade")))
        arguments = ["--project", str(self.project), "--home", str(self.home), "--exact", "--json", "--watch"]
        document = json.loads(subprocess.check_output([
            str(binary), "_backend", "helper-read", "--provider", "fileblade.core.hooks",
            "--plugin-dir", "", "--helper", "inventory", "--method", "list", "--arguments", json.dumps(arguments)
        ], env={"PATH": os.environ["PATH"], "HOME": str(self.home), "XDG_STATE_HOME": str(self.base / "state")}))
        self.assertTrue(document["ok"])
        self.assertIn(str(self.project), document["watchPaths"])
        self.assertEqual(document["items"], [])

    def test_overdeep_source_is_skipped_without_abandoning_other_agents(self):
        source = self.project / ".claude/settings.json"
        source.parent.mkdir()
        source.write_text('{"hooks":' + '[' * 2000 + '0' + ']' * 2000 + '}')
        other = self.project / ".codex/hooks.json"
        other.parent.mkdir()
        other.write_text('{"hooks":{"Stop":[{"command":"private-command"}]}}')
        document, paths = self.scan()
        self.assertTrue(any(row["agent"] == "codex" for row in document["items"]))
        self.assertIn(source.parent, paths)

    def test_overdeep_json_and_toml_reads_are_controlled_refusals(self):
        from agent_hooks import apply, safeio
        for suffix, text in (("json", '{"value":' + '[' * 2000 + '0' + ']' * 2000 + '}'),
                             ("toml", 'value = ' + '[' * 2000 + '0' + ']' * 2000)):
            source = self.project / ("deep." + suffix)
            source.write_text(text)
            reader = safeio.load_json if suffix == "json" else safeio.load_toml
            self.assertIsNone(reader(source))
            if suffix == "json":
                self.assertIsInstance(apply.load_target(source), str)
            self.assertEqual(source.read_text(), text)

    def test_parser_recursion_exceptions_are_controlled_refusals(self):
        from agent_hooks import apply, safeio
        source = self.project / "settings.json"
        source.write_text('{}')
        with patch.object(safeio.json, "loads", side_effect=RecursionError):
            self.assertIsNone(safeio.load_json(source))
            self.assertIsInstance(apply.load_target(source), str)
        with patch.object(safeio.tomllib, "loads", side_effect=RecursionError):
            self.assertIsNone(safeio.load_toml(source))


if __name__ == "__main__":
    unittest.main()
