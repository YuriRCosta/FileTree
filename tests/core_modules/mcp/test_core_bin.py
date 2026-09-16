import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
KIND = 'mcp'
PROVIDER = "fileblade.core." + KIND


class CoreBinLifecycle(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="fileblade-bin-lifecycle-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home = self.root / "home"
        self.project = self.home / "project"
        self.project.mkdir(parents=True)
        self.binary = os.environ.get("FILEBLADE_BINARY", str(ROOT / "target/release/fileblade"))
        self.env = dict(os.environ, HOME=str(self.home), XDG_CONFIG_HOME=str(self.home / ".config"),
                        XDG_STATE_HOME=str(self.home / ".local/state"), XDG_CACHE_HOME=str(self.home / ".cache"),
                        XDG_DATA_HOME=str(self.home / ".local/share"),
                        CODEX_HOME=str(self.home / ".codex"), CLAUDE_CONFIG_DIR=str(self.home / ".claude"),
                        FILEBLADE_BINARY=self.binary, FILEBLADE_APP_ROOT=str(ROOT), PYTHONDONTWRITEBYTECODE="1")
        self.source = self.project / (".mcp.json" if KIND == "mcp" else ".claude/settings.json")
        self.source.parent.mkdir(exist_ok=True)
        self.original = ({"mcpServers": {"fixture": {"command": "printf", "args": ["fixture"]}}}
                         if KIND == "mcp" else {"hooks": {"PreToolUse": [{"hooks": [{"type": "command", "command": "printf fixture"}]}]}})
        manifest = json.loads((ROOT / "modules" / KIND / "source.json").read_text())
        helper = manifest["helpers"][0]["id"]
        self.route = json.dumps({"provider": PROVIDER, "directory": "", "helper": helper})
        self.store = self.home / (".local/state/fileblade/" + KIND + "-recovery")
        self.reset_source()

    def reset_source(self):
        self.source.write_text(json.dumps(self.original))

    def backend(self, *args):
        result = subprocess.run([self.binary, "_backend", *args], env=self.env,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=20)
        if not result.stdout:
            return {"ok": False, "error": result.stderr.decode()}
        return json.loads(result.stdout)

    def remove(self):
        result = subprocess.run([str(ROOT / ("python/bin/agent-" + KIND + "ctl")), "list", "--project", str(self.project), "--json"],
                                env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=20, check=True)
        document = json.loads(result.stdout)
        rows = document["definitions"] if KIND == "mcp" else document["items"]
        row = rows[0]
        response = self.backend("bin-remove", "--module", KIND, "--item", json.dumps({"id": row["id"], "name": "Fixture"}),
                                "--helper-route", self.route, "--arguments", json.dumps(["--project", str(self.project), "--id", row["id"], "--json"]))
        self.assertTrue(response.get("ok"), response)
        return response["entry"]

    def test_remove_and_purge_past_the_store_capacity_leaves_no_hidden_recovery(self):
        for _ in range(70):
            entry = self.remove()
            self.assertEqual(len(list(self.store.glob("*.json"))), 1)
            response = self.backend("bin-purge", "--module", KIND, "--id", entry)
            self.assertTrue(response.get("ok"), response)
            self.assertEqual(list(self.store.glob("*.json")), [])
            self.assertEqual(self.backend("bin-list", "--module", KIND)["items"], [])
            self.reset_source()

    def test_legacy_recovery_route_uses_bundled_helper_without_its_checkout(self):
        self.route = json.dumps({"provider": "data-goblin.fileblade-" + KIND,
                                 "directory": str(self.home / "retired-companion"), "helper": "inventory"})
        entry = self.remove()
        response = self.backend("bin-restore", "--module", KIND, "--id", entry)
        self.assertTrue(response.get("ok"), response)
        self.assertEqual(json.loads(self.source.read_text()), self.original)
        self.assertFalse((self.home / "retired-companion").exists())

    def test_unavailable_runtime_preserves_both_records_until_repaired(self):
        entry = self.remove()
        removed = self.source.read_bytes()
        self.env["FILEBLADE_APP_ROOT"] = str(self.home / "missing-runtime")
        response = self.backend("bin-restore", "--module", KIND, "--id", entry)
        self.assertFalse(response.get("ok"), response)
        self.assertEqual(self.source.read_bytes(), removed)
        self.assertEqual(len(list(self.store.glob("*.json"))), 1)
        self.assertEqual(len(self.backend("bin-list", "--module", KIND)["items"]), 1)
        self.env["FILEBLADE_APP_ROOT"] = str(ROOT)
        response = self.backend("bin-restore", "--module", KIND, "--id", entry)
        self.assertTrue(response.get("ok"), response)
        self.assertEqual(json.loads(self.source.read_text()), self.original)
        self.assertEqual(list(self.store.glob("*.json")), [])


if __name__ == "__main__":
    unittest.main()
