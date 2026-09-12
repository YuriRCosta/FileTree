import os
from pathlib import Path
import subprocess
import sys
import tempfile


root = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix="fileblade-core-contracts-") as temporary:
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", PYTHONPATH=str(root / "python"),
                       XDG_STATE_HOME=temporary)
    cases = {
        "skills": ["unit.py"],
        "memory": ["test_discovery.py", "test_apply.py"],
        "hooks": ["test_discovery.py", "test_apply.py", "test_undo.py", "test_recovery_store.py"],
        "mcp": ["test_inventory.py", "test_apply.py", "test_undo.py", "test_recovery_store.py"],
    }
    for module, files in cases.items():
        for filename in files + ["test_path_identity.py", "test_watch.py"] + (["test_core_bin.py"] if module in ("hooks", "mcp") else []):
            path = Path(__file__).parent / module / filename
            print(f"{module}/{filename}", flush=True)
            subprocess.run([sys.executable, "-B", str(path)], cwd=root, env=environment,
                           check=True, timeout=120)
