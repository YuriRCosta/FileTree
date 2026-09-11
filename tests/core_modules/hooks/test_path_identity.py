import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "python"))
import agent_hooks
from fileblade_paths import display, parse_path, path_text


class NativePaths(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.home = self.base / "home"
        self.home.mkdir()
        self.project = self.base / os.fsdecode(b"repo-\xff")
        self.hooks = self.project / ".github" / "hooks"
        self.hooks.mkdir(parents=True)
        self.names = [os.fsdecode(b"\xff.json"), os.fsdecode(b"\xfe.json"), "\ufffd.json", "\\xFF.json"]
        self.original = {"version": 1, "hooks": {"preToolUse": [{"type": "command", "bash": "printf TEST_NOT_EXECUTED"}]}}
        for name in self.names:
            (self.hooks / name).write_text(json.dumps(self.original))
        self.env = {"PATH": os.environ["PATH"], "HOME": str(self.home), "PYTHONDONTWRITEBYTECODE": "1",
                    "XDG_DATA_HOME": str(self.base / "data"), "XDG_STATE_HOME": str(self.base / "state")}

    def run_helper(self, command, *arguments, payload=None):
        locations = [] if command == "restore" else ["--exact", "--project", path_text(str(self.project)), "--home", str(self.home)]
        result = subprocess.run([str(ROOT / "python/bin/agent-hooksctl"), command, "--json", *locations, *arguments],
                                input=None if payload is None else json.dumps(payload).encode() + b"\n",
                                env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        return json.loads(result.stdout.decode("utf-8"))

    def core(self, command, *arguments):
        raw = subprocess.check_output([str(ROOT / "target/release/fileblade"), "_backend", command, *arguments], env=self.env)
        return json.loads(raw)

    def test_distinct_paths_names_and_ids(self):
        document = self.run_helper("list")
        self.assertEqual(document["project"], path_text(str(self.project)))
        rows = [row for row in document["items"] if row["agent"] == "copilot-cli" and row["scope"] == "project"]
        self.assertEqual(len(rows), 4)
        self.assertEqual(len({row["id"] for row in rows}), 4)
        self.assertEqual({row["source"]["name"] for row in rows}, {display(name) for name in self.names})
        self.assertEqual({parse_path(row["source"]["path"]) for row in rows}, {str(self.hooks / name) for name in self.names})
        self.assertNotIn("TEST_NOT_EXECUTED", json.dumps(document))

    def test_native_source_undo_through_core_bin(self):
        source = self.hooks / self.names[0]
        row = next(row for row in self.run_helper("list")["items"] if parse_path(row["source"]["path"]) == str(source))
        removed = self.run_helper("remove", "--id", row["id"])
        self.assertTrue(removed["ok"], removed)
        self.assertEqual(removed["payload"]["source"], path_text(str(source)))
        self.assertEqual(removed["results"][0]["touched"], [path_text(str(source))])
        item = {"id": row["id"], "name": row["source"]["name"], "path": row["source"]["path"], "paths": [], "payload": removed["payload"]}
        stored = self.core("bin-put", "--module", "hooks", "--item", json.dumps(item))
        self.assertTrue(stored["ok"], stored)
        record = self.core("bin-restore", "--module", "hooks", "--id", stored["entry"])
        self.assertTrue(record["ok"], record)
        restored = self.run_helper("restore", "--record-id", stored["entry"], "--payload-stdin", payload=record["payload"])
        self.assertTrue(restored["ok"], restored)
        for name in self.names:
            self.assertEqual(json.loads((self.hooks / name).read_text()), self.original)

    def test_unrepresentable_undo_refuses_before_removal(self):
        source = self.hooks / self.names[0]
        invalid = {"version": 1, "hooks": {"preToolUse": [{"type": "command", "bash": "printf \udcff"}]}}
        source.write_text(json.dumps(invalid))
        before = source.read_bytes()
        row = next(row for row in self.run_helper("list")["items"] if parse_path(row["source"]["path"]) == str(source))
        removed = self.run_helper("remove", "--id", row["id"])
        self.assertFalse(removed["ok"], removed)
        self.assertEqual(source.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
