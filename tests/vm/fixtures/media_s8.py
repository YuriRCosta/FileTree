from pathlib import Path
import sys

root = Path(sys.argv[1])
(root / "nested").mkdir(parents=True, exist_ok=True)
(root / "visible.txt").write_text("visible\n")
(root / ".hidden.txt").write_text("hidden\n")
for index in range(300):
    (root / "nested" / f"entry-{index:04d}.txt").write_text("search fixture\n")
