import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "python"))
import agent_skills
from fileblade_paths import display, parse_path, path_text


class NativePaths(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.home = self.base / "home"
        self.home.mkdir()
        self.project = self.base / os.fsdecode(b"repo-\xff")
        self.skills = self.project / ".claude" / "skills"
        self.skills.mkdir(parents=True)
        self.names = [os.fsdecode(b"\xff"), os.fsdecode(b"\xfe"), "\ufffd", "\\xFF"]
        for name in self.names:
            directory = self.skills / name
            directory.mkdir()
            (directory / "SKILL.md").write_text("# original\n")
        self.env = {"PATH": os.environ["PATH"], "HOME": str(self.home), "PYTHONDONTWRITEBYTECODE": "1"}
        if "FILEBLADE_BINARY" in os.environ:
            self.env["FILEBLADE_BINARY"] = os.environ["FILEBLADE_BINARY"]

    def run_helper(self, command, *arguments):
        raw = subprocess.check_output([str(ROOT / "python/bin/agent-skillsctl"), command, "--json", "--exact",
                                       "--project", path_text(str(self.project)), "--home", str(self.home),
                                       "--prefix", str(self.base / "etc"), *arguments], env=self.env)
        return json.loads(raw.decode("utf-8"))

    def test_distinct_paths_names_and_ids(self):
        document = self.run_helper("list")
        self.assertEqual(document["project"], path_text(str(self.project)))
        rows = document["items"]
        self.assertEqual(len(rows), 4)
        self.assertEqual(len({row["id"] for row in rows}), 4)
        self.assertEqual({row["name"] for row in rows}, {display(name) for name in self.names})
        self.assertEqual({parse_path(row["path"]) for row in rows}, {str(self.skills / name / "SKILL.md") for name in self.names})
        roots = self.run_helper("roots")["roots"]
        self.assertIn(str(self.skills), {parse_path(row["path"]) for row in roots})

    def test_apply_unapply_preserves_native_skill_directory(self):
        source = self.skills / self.names[0]
        row = next(row for row in self.run_helper("list")["items"] if parse_path(row["path"]) == str(source / "SKILL.md"))
        result = self.run_helper("apply", "--id", row["id"], "--agent", "codex", "--state", "on")
        self.assertTrue(result["ok"], result)
        link = self.project / ".agents" / "skills" / self.names[0]
        self.assertEqual(os.readlink(link), str(source))
        self.assertEqual(result["results"][0]["touched"], [path_text(str(link))])
        self.assertTrue(self.run_helper("apply", "--id", row["id"], "--agent", "codex", "--state", "off")["ok"])
        self.assertFalse(link.exists())
        for name in self.names:
            self.assertEqual((self.skills / name / "SKILL.md").read_text(), "# original\n")


if __name__ == "__main__":
    unittest.main()
