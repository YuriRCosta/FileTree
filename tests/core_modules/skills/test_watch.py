import os
from pathlib import Path
import stat
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_skills import apply, bounds, discovery
from fileblade_inventory import WatchPlan


class SkillsWatchTests(unittest.TestCase):
    def test_new_roots_descriptors_and_link_targets_are_watched(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            home, project, vault = base / "home", base / "project", base / "vault"
            home.mkdir(); project.mkdir(); vault.mkdir()
            env = discovery.Environment(str(home), str(project), exact=True, prefix=str(base / "prefix"), enforce_secure_system=False)
            with WatchPlan() as plan:
                discovery.collect(env)
            self.assertIn(project, plan.paths)
            self.assertIn(home, plan.paths)
            skill = project / ".claude" / "skills" / "example"
            skill.parent.mkdir(parents=True)
            skill.symlink_to(vault, target_is_directory=True)
            with WatchPlan() as plan:
                self.assertEqual(discovery.collect(env)["items"], [])
            self.assertIn(vault, plan.paths)
            self.assertIn(skill.parent, plan.paths)
            (vault / "SKILL.md").write_text("---\nname: example\ndescription: watch me\n---\n")
            with WatchPlan() as plan:
                document = plan.finish(discovery.collect(env))
            self.assertEqual([row["name"] for row in document["items"]], ["example"])
            self.assertIn(str(vault), document["watchPaths"])
            self.assertLessEqual(len(document["watchPaths"]), 512)

    def test_directory_input_is_bounded_before_sorting_even_without_skills(self):
        class Scan:
            def __enter__(self): return self
            def __exit__(self, *args): pass
            def __iter__(self):
                for index in range(5):
                    yield type("Entry", (), {"name": str(index)})()
                raise AssertionError("read past limit plus overflow entry")
        with patch.object(bounds.os, "scandir", return_value=Scan()):
            names, truncated = bounds.bounded_names("/missing", 4)
        self.assertEqual(names, ["0", "1", "2", "3"])
        self.assertTrue(truncated)

    def test_many_sibling_files_do_not_hide_the_descriptor(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index in range(100):
                (root / str(index)).touch()
            descriptor = root / "SKILL.md"
            descriptor.write_text("skill")
            with patch.object(discovery.os, "scandir", side_effect=AssertionError("descriptor must not enumerate siblings")):
                self.assertEqual(discovery.descriptor(str(root)), str(descriptor))

    def test_secure_read_checks_the_open_descriptor_not_an_earlier_path(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.json"
            path.write_text("private")
            metadata = os.stat(path)
            protected = os.stat_result((stat.S_IFREG | 0o600, metadata.st_ino, metadata.st_dev, 1, 0, 0, metadata.st_size, 0, 0, 0))
            writable = os.stat_result((stat.S_IFREG | 0o666, metadata.st_ino, metadata.st_dev, 1, 0, 0, metadata.st_size, 0, 0, 0))
            with patch.object(bounds.os, "fstat", return_value=protected):
                self.assertEqual(bounds.read_secure_document(str(path), True), "private")
            with patch.object(bounds.os, "fstat", return_value=writable):
                self.assertEqual(bounds.read_secure_document(str(path), True), "")

    def test_unlink_refuses_an_incomplete_directory_scan_before_any_change(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, target = base / "skills", base / "target"
            root.mkdir(); target.mkdir()
            (root / "alias").symlink_to(target)
            env = discovery.Environment(str(base))
            with patch.object(apply, "bounded_names", return_value=(["alias"], True)):
                outcome = apply.unlink_off(env, "codex", "user", [str(root)], str(target))
            self.assertFalse(outcome.ok)
            self.assertFalse(outcome.changed)
            self.assertTrue((root / "alias").is_symlink())


if __name__ == "__main__":
    unittest.main()
