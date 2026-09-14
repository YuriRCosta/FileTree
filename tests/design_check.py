import json
import os
import re
import sys

RULE = "no-rounded-corners"
REGISTRY = "tools/design-exceptions.json"
BINDING = re.compile(r"^\s*(radius|topLeftRadius|topRightRadius|bottomLeftRadius|bottomRightRadius)\s*:\s*(.+?)\s*$")
SKIPPED = ("tests/", "target/", ".git/")


def rounded(value):
    return value not in ("0", "0.0")


def registry():
    try:
        with open(REGISTRY, encoding="utf-8") as handle:
            document = json.load(handle)
    except FileNotFoundError:
        return {}
    allowed = {}
    for entry in document.get(RULE, []):
        allowed.setdefault(entry["path"], {})[entry["binding"]] = entry["reason"]
    return allowed


def sources(arguments):
    if arguments:
        return [path for path in arguments if path.endswith(".qml")]
    found = []
    for directory, subdirectories, names in os.walk("."):
        subdirectories[:] = [name for name in subdirectories if name not in (".git", "target", "tests")]
        for name in names:
            if name.endswith(".qml"):
                found.append(os.path.relpath(os.path.join(directory, name), "."))
    return sorted(found)


def violations(paths, allowed):
    found = []
    for path in paths:
        relative = os.path.relpath(path, ".")
        if relative.startswith(SKIPPED) or not os.path.isfile(relative):
            continue
        with open(relative, encoding="utf-8") as handle:
            for number, line in enumerate(handle, 1):
                match = BINDING.match(line.rstrip("\n"))
                if not match or not rounded(match.group(2)):
                    continue
                binding = line.strip()
                if binding in allowed.get(relative, {}):
                    continue
                found.append((relative, number, binding))
    return found


def hook_paths():
    try:
        payload = json.load(sys.stdin)
    except (json.JSONDecodeError, ValueError):
        return []
    tool = payload.get("tool_input") or {}
    candidates = [tool.get("file_path")]
    for edit in tool.get("edits") or []:
        candidates.append(edit.get("file_path"))
    return [path for path in candidates if path]


def main(arguments):
    hook = "--hook" in arguments
    paths = hook_paths() if hook else [value for value in arguments if not value.startswith("-")]
    if hook and not paths:
        return 0
    found = violations(sources(paths), registry())
    if not found:
        return 0
    stream = sys.stderr if hook else sys.stdout
    print(
        f"{RULE}: corners stay square unless the surface is registered as round by design.",
        file=stream,
    )
    for path, number, binding in found:
        print(f"  {path}:{number}: {binding}", file=stream)
    print(
        f"Remove the binding so the corner stays square, or register the surface in {REGISTRY} "
        "with the reason it is round by design.",
        file=stream,
    )
    return 2 if hook else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
