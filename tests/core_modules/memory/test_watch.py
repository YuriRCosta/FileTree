from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_memory import adapters, common, discovery
from fileblade_inventory import WatchPlan


class MemoryWatchTests(unittest.TestCase):
    def test_linked_auto_memory_directories_remain_discoverable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home, external = root / "home", root / "external"
            projects = home / ".claude" / "projects"
            projects.mkdir(parents=True)
            (external / "memory").mkdir(parents=True)
            (projects / "linked").symlink_to(external, target_is_directory=True)
            self.assertEqual(adapters.auto_memory_roots({"home": home}), [projects / "linked" / "memory"])
            self.assertEqual(common.bounded_directories(projects, 10), [])

    def test_empty_project_and_later_nested_rule_are_watched(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            home, project = base / "home", base / "project"
            home.mkdir(); project.mkdir()
            def scan():
                with WatchPlan() as plan:
                    document = discovery.collect(str(project), str(home), environ={"HOME": str(home)}, exact=True)
                    return document, plan
            _, initial = scan()
            self.assertIn(project, initial.paths)
            rule = project / ".claude" / "rules" / "nested" / "new.md"
            rule.parent.mkdir(parents=True)
            rule.write_text("new rule")
            document, plan = scan()
            self.assertIn(rule.parent, plan.paths)
            self.assertTrue(any(str(row["path"]) == str(rule) for row in document["items"]))

    def test_glob_watches_empty_matching_subdirectories_and_stays_contained(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, outside = base / "docs", base / "docs-sibling"
            nested = root / "nested"
            nested.mkdir(parents=True); outside.mkdir()
            (outside / "private.md").write_text("outside")
            (root / "alias").symlink_to(outside, target_is_directory=True)
            (root / "loop").symlink_to(root, target_is_directory=True)
            with WatchPlan() as plan:
                self.assertEqual(common.bounded_glob(root, "**/*.md"), [])
            self.assertIn(nested, plan.paths)
            self.assertNotIn(outside, plan.paths)
            (nested / "new.md").write_text("new")
            self.assertEqual(common.bounded_glob(root, "**/*.md"), [nested / "new.md"])

    def test_glob_bounds_directory_work_even_without_matches(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for number in range(10):
                (root / str(number)).mkdir()
            with patch.object(common, "MAX_DISCOVERY_DIRS", 3), patch.object(common.os, "scandir", wraps=common.os.scandir) as scanned:
                self.assertEqual(common.bounded_glob(root, "**/*.absent"), [])
                self.assertLessEqual(scanned.call_count, 3)


if __name__ == "__main__":
    unittest.main()
