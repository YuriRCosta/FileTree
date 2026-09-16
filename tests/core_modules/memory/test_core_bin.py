import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
KIND = "memory"
CONTENT = "# Fixture memory\n\nRemember the rivet.\n"


class CoreBinLifecycle(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="fileblade-bin-lifecycle-")
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name) / "home"
        self.binary = os.environ.get("FILEBLADE_BINARY", str(ROOT / "target/release/fileblade"))
        self.env = dict(os.environ, HOME=str(self.home), XDG_CONFIG_HOME=str(self.home / ".config"),
                        XDG_STATE_HOME=str(self.home / ".local/state"), XDG_CACHE_HOME=str(self.home / ".cache"),
                        XDG_DATA_HOME=str(self.home / ".local/share"),
                        CODEX_HOME=str(self.home / ".codex"), CLAUDE_CONFIG_DIR=str(self.home / ".claude"),
                        FILEBLADE_BINARY=self.binary, FILEBLADE_APP_ROOT=str(ROOT), PYTHONDONTWRITEBYTECODE="1")
        self.fixture = self.home / ".claude/CLAUDE.md"
        self.fixture.parent.mkdir(parents=True)
        self.fixture.write_text(CONTENT)
        self.store = self.home / ".local/share/fileblade/bin" / KIND
        self.consent(True)

    def consent(self, enabled):
        response = self.backend("preferences-set", "--agent-management", "true" if enabled else "false")
        self.assertTrue(response.get("ok"), response)

    def backend(self, *args):
        result = subprocess.run([self.binary, "_backend", *args], env=self.env,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
        if not result.stdout:
            return {"ok": False, "error": result.stderr.decode()}
        return json.loads(result.stdout)

    def listed(self):
        result = subprocess.run([str(ROOT / ("python/bin/agent-" + KIND + "ctl")), "list", "--json"], env=self.env,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30, check=True)
        document = json.loads(result.stdout)
        rows = [item for item in document["items"] if str(item.get("path")) == str(self.fixture)]
        self.assertEqual(len(rows), 1, document)
        return rows[0]

    def item(self):
        row = self.listed()
        self.assertIsNone(row.get("inlineBytes"), row)
        return json.dumps({"id": row["id"], "name": row.get("name", ""), "kind": str(row.get("kind", "")),
                           "scope": row.get("scope", ""), "detail": "", "path": str(self.fixture),
                           "realpath": str(row.get("realpath") or ""), "paths": [str(self.fixture)],
                           "position": 0, "groups": ["User"]})

    def remove(self):
        response = self.backend("bin-put", "--module", KIND, "--item", self.item())
        self.assertTrue(response.get("ok"), response)
        return response["entry"]

    def entries(self):
        document = self.backend("bin-list", "--module", KIND)
        self.assertTrue(document.get("ok"), document)
        return document["items"]

    def test_delete_moves_the_memory_file_into_the_bin_and_restore_puts_it_back_unchanged(self):
        entry = self.remove()
        self.assertFalse(self.fixture.exists())
        rows = self.entries()
        self.assertEqual([row["id"] for row in rows], [entry])
        self.assertEqual(rows[0]["kind"], "bin")
        self.assertEqual(rows[0]["path"], str(self.fixture))
        self.assertTrue((self.store / entry.removeprefix("bin:") / "manifest.json").is_file())
        response = self.backend("bin-restore", "--module", KIND, "--id", entry)
        self.assertTrue(response.get("ok"), response)
        self.assertEqual(self.fixture.read_text(), CONTENT)
        self.assertEqual(self.entries(), [])
        self.assertEqual(self.listed()["path"], str(self.fixture))

    def test_discard_is_the_only_step_that_destroys_the_memory_file(self):
        entry = self.remove()
        record = self.store / entry.removeprefix("bin:")
        self.assertTrue(record.is_dir())
        response = self.backend("bin-purge", "--module", KIND, "--id", entry)
        self.assertTrue(response.get("ok"), response)
        self.assertFalse(record.exists())
        self.assertFalse(self.fixture.exists())
        self.assertEqual(self.entries(), [])

    def test_removal_is_refused_and_the_file_untouched_while_management_is_disabled(self):
        item = self.item()
        self.consent(False)
        response = self.backend("bin-put", "--module", KIND, "--item", item)
        self.assertFalse(response.get("ok"), response)
        self.assertIn("Manage agent files", response.get("message", ""))
        self.assertEqual(self.fixture.read_text(), CONTENT)
        self.assertEqual(self.entries(), [])

    def test_a_refused_restore_keeps_the_record_until_management_is_enabled(self):
        entry = self.remove()
        self.consent(False)
        response = self.backend("bin-restore", "--module", KIND, "--id", entry)
        self.assertFalse(response.get("ok"), response)
        self.assertIn("Manage agent files", response.get("message", ""))
        self.assertTrue((self.store / entry.removeprefix("bin:") / "manifest.json").is_file())
        self.assertFalse(self.fixture.exists())
        self.consent(True)
        response = self.backend("bin-restore", "--module", KIND, "--id", entry)
        self.assertTrue(response.get("ok"), response)
        self.assertEqual(self.fixture.read_text(), CONTENT)


if __name__ == "__main__":
    unittest.main()
