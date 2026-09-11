import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "python"))
from agent_memory import common
from fileblade_paths import display, parse_path, path_text


class NativePaths(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.home = self.base / "home"
        self.home.mkdir()
        self.project = self.base / os.fsdecode(b"repo-\xff")
        (self.project / ".git").mkdir(parents=True)
        self.source = self.project / "AGENTS.md"
        self.source.write_text("# original\n")
        self.rules = self.project / ".claude" / "rules"
        self.rules.mkdir(parents=True)
        self.names = [os.fsdecode(b"\xff.md"), os.fsdecode(b"\xfe.md"), "\ufffd.md", "\\xFF.md"]
        for name in self.names:
            (self.rules / name).write_text("# rule\n")
        self.env = {"PATH": os.environ["PATH"], "HOME": str(self.home), "PYTHONDONTWRITEBYTECODE": "1"}

    def run_helper(self, command, *arguments):
        raw = subprocess.check_output([str(ROOT / "python/bin/agent-memoryctl"), command, "--json", "--exact",
                                       "--project", path_text(str(self.project)), "--home", str(self.home),
                                       "--prefix", str(self.base / "etc"), *arguments], env=self.env)
        return json.loads(raw.decode("utf-8"))

    def test_distinct_paths_names_and_ids(self):
        document = self.run_helper("list")
        self.assertEqual(document["project"], path_text(str(self.project)))
        rows = [row for row in document["items"] if Path(parse_path(row["path"])).parent == self.rules]
        self.assertEqual(len(rows), 4)
        self.assertEqual(len({row["id"] for row in rows}), 4)
        self.assertEqual({row["name"] for row in rows}, {display(name) for name in self.names})
        self.assertEqual({parse_path(row["path"]) for row in rows}, {str(self.rules / name) for name in self.names})
        self.assertNotEqual(common.stable_id(str(self.rules / self.names[0])), common.stable_id(str(self.rules / self.names[1])))

    def test_link_unlink_native_project_without_touching_neighbor(self):
        neighbor = self.base / "repo-\ufffd"
        neighbor.mkdir()
        (neighbor / "AGENTS.md").write_text("neighbor")
        row = next(row for row in self.run_helper("list")["items"] if parse_path(row["path"]) == str(self.source))
        result = self.run_helper("apply", "--id", row["id"], "--agent", "claude-code", "--state", "on")
        self.assertTrue(result["ok"], result)
        link = self.project / "CLAUDE.md"
        self.assertEqual(os.readlink(link), str(self.source))
        self.assertEqual(result["results"][0]["touched"], [path_text(str(link))])
        self.assertTrue(self.run_helper("apply", "--id", row["id"], "--agent", "claude-code", "--state", "off")["ok"])
        self.assertFalse(link.exists())
        self.assertEqual(self.source.read_text(), "# original\n")
        self.assertEqual((neighbor / "AGENTS.md").read_text(), "neighbor")


if __name__ == "__main__":
    unittest.main()
