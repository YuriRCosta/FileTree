from __future__ import annotations

import os
from pathlib import Path

SKILL = "---\nname: {name}\ndescription: {description}\n---\n\nBody for {name}.\n"

def write_skill(root: Path, name: str, description: str = "A fixture skill") -> Path:
    directory = root / name
    directory.mkdir(parents=True, exist_ok=True)
    descriptor = directory / "SKILL.md"
    descriptor.write_text(SKILL.format(name=name, description=description), encoding="utf-8")
    return descriptor

def link_skill(root: Path, name: str, target: Path) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    link = root / name
    if not link.exists():
        os.symlink(target, link)
    return link
