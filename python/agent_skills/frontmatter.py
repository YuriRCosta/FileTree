from __future__ import annotations

import re

from .bounds import MAX_FRONTMATTER_BYTES, clean

DELIMITER = re.compile(r"^---[ \t]*\r?\n")
TERMINATOR = re.compile(r"^---[ \t]*\r?$")

def block(text: str) -> str:
    head = text[:MAX_FRONTMATTER_BYTES]
    if not DELIMITER.match(head):
        return ""
    body = head.split("\n", 1)
    if len(body) < 2:
        return ""
    lines = body[1].split("\n")
    collected: list[str] = []
    for line in lines:
        if TERMINATOR.match(line):
            return "\n".join(collected)
        collected.append(line)
    return ""

def value(text: str, key: str) -> str:
    source = block(text)
    if not source:
        return ""
    pattern = re.compile(rf"^{re.escape(key)}[ \t]*:[ \t]*(.*)$", re.MULTILINE)
    found = pattern.search(source)
    if not found:
        return ""
    first = found.group(1).strip()
    lines = source[found.end():].split("\n")[1:]
    if first in (">", "|", ">-", "|-", ">+", "|+"):
        first = ""
        folded: list[str] = []
        for line in lines:
            if line.strip() and not line[:1].isspace():
                break
            folded.append(line.strip())
        return clean(" ".join(folded), 1 << 16)
    if first.startswith("#"):
        return ""
    first = re.sub(r"\s+#.*$", "", first)
    return clean(first.strip().strip('"').strip("'"), 1 << 16)
