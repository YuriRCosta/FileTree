from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from fileblade_paths import NativePath

from .redaction import clean_label
from .safeio import expanded, load_json

MAX_LABELS = 512
DIGEST = re.compile(r"[0-9a-f]{8}")

def labels_path(home: Path, environ: dict[str, str]) -> Path:
    configured = environ.get("XDG_CONFIG_HOME", "")
    base = expanded(configured) if configured else home / ".config"
    return base / "omarchy" / "fileblade" / "hooks" / "labels.json"

def load_labels(path: Path) -> dict[str, str]:
    document = load_json(path)
    raw = document.get("labels") if isinstance(document, dict) else None
    labels: dict[str, str] = {}
    if not isinstance(raw, dict):
        return labels
    for digest, text in list(raw.items())[:MAX_LABELS]:
        if isinstance(digest, str) and DIGEST.fullmatch(digest) and isinstance(text, str):
            cleaned = clean_label(text)
            if cleaned:
                labels[digest] = cleaned
    return labels

def attach_labels(rows: list[dict[str, Any]], labels: dict[str, str]) -> None:
    for entry in rows:
        summary = entry.get("summary")
        if not isinstance(summary, dict):
            continue
        text = labels.get(str(summary.get("digest", "")))
        if text:
            summary["label"] = text
            summary["labelSource"] = "user"

def set_label(home: Path, environ: dict[str, str], digest: str, text: str) -> dict[str, Any]:
    from .apply import write_atomic
    if not isinstance(digest, str) or not DIGEST.fullmatch(digest):
        return {"ok": False, "schemaVersion": 1, "digest": "", "label": "", "changed": False, "message": "digest must be eight hex characters", "touched": []}
    cleaned = clean_label(text)
    path = labels_path(home, environ)
    labels = load_labels(path)
    if len(labels) >= MAX_LABELS and cleaned and digest not in labels:
        return {"ok": False, "schemaVersion": 1, "digest": digest, "label": "", "changed": False, "message": "the label store is full", "touched": []}
    changed = labels.get(digest, "") != cleaned
    if changed:
        if cleaned:
            labels[digest] = cleaned
        else:
            labels.pop(digest, None)
        try:
            write_atomic(path, {"version": 1, "labels": labels})
        except OSError as error:
            return {"ok": False, "schemaVersion": 1, "digest": digest, "label": cleaned, "changed": False, "message": str(error), "touched": []}
    return {"ok": True, "schemaVersion": 1, "digest": digest, "label": cleaned, "changed": changed, "message": "",
            "touched": [NativePath(str(path))] if changed else []}
