import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def snapshot(directory):
    return {str(path.relative_to(directory)): path.read_bytes() for path in directory.rglob("*") if path.is_file()}


def check(repository, package, launcher, installed):
    core = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="fileblade-readonly-") as temporary:
        base = Path(temporary)
        plugins = base / "plugins"
        identifier = json.loads((repository / "manifest.json").read_text())["id"]
        provider = plugins / (identifier if installed else repository.name)
        dependency = plugins / ("data-goblin.fileblade" if installed else "fileblade")
        shutil.copytree(repository / package, provider / package, ignore=shutil.ignore_patterns("__pycache__"))
        shutil.copytree(core / "python", dependency / "python", ignore=shutil.ignore_patterns("__pycache__"))
        (provider / "bin").mkdir()
        shutil.copy2(repository / "bin" / launcher, provider / "bin" / launcher)
        before = snapshot(plugins)
        result = subprocess.run([str(provider / "bin" / launcher), "--help"],
                                env={"PATH": os.environ["PATH"], "HOME": str(base / "home")},
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        assert result.returncode == 0, result.stderr.decode("utf-8", "replace")
        assert snapshot(plugins) == before, "helper imports modified the plugin tree (including bytecode caches)"


def check_core():
    core = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="fileblade-core-readonly-") as temporary:
        base = Path(temporary)
        runtime = base / "runtime"
        shutil.copytree(core / "python", runtime / "python", ignore=shutil.ignore_patterns("__pycache__"))
        before = snapshot(runtime)
        for name in ("skills", "memory", "hooks", "mcp"):
            result = subprocess.run([str(runtime / "python" / "bin" / ("agent-" + name + "ctl")), "--help"],
                                    env={"PATH": os.environ["PATH"], "HOME": str(base / "home")},
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10, check=False)
            assert result.returncode == 0, result.stderr.decode("utf-8", "replace")
        assert snapshot(runtime) == before, "core helper imports modified the runtime"


if __name__ == "__main__":
    if len(sys.argv) == 1:
        check_core()
        print("read-only core helper imports: ok (standalone runtime)")
    else:
        repository = Path(sys.argv[1]).resolve()
        for installed in (False, True):
            check(repository, sys.argv[2], sys.argv[3], installed)
        print("read-only helper imports: ok (development and installed layouts)")
