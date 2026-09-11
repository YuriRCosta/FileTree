from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any

from fileblade_paths import NativePath, parse_path, wire
from fileblade_inventory import SCOPES, WatchPlan

from . import apply as applying, discovery, registry

MAX_OUTPUT_BYTES = 1024 * 1024

def serialized(value: dict[str, Any]) -> bytes:
    return (json.dumps(wire(value), ensure_ascii=True, separators=(",", ":")) + "\n").encode("utf-8")

def encoded(payload: dict[str, Any]) -> bytes:
    def document(items: list[Any], truncated: bool) -> bytes:
        value = dict(payload)
        value["items"] = items
        value["count"] = len(items)
        value["truncated"] = bool(value.get("truncated")) or truncated
        return serialized(value)

    items = payload.get("items")
    if not isinstance(items, list):
        return serialized(payload)
    data = document(items, False)
    if len(data) <= MAX_OUTPUT_BYTES:
        return data
    low, high = 0, len(items)
    best = document([], True)
    while low <= high:
        middle = (low + high) // 2
        candidate = document(items[:middle], True)
        if len(candidate) <= MAX_OUTPUT_BYTES:
            best = candidate
            low = middle + 1
        else:
            high = middle - 1
    return best

def emit(payload: dict[str, Any]) -> None:
    sys.stdout.buffer.write(encoded(payload))
    sys.stdout.buffer.flush()

def environment(args: argparse.Namespace) -> discovery.Environment:
    home = parse_path(args.home) if args.home else os.path.expanduser("~")
    return discovery.Environment(
        home=os.path.abspath(home),
        anchor=parse_path(args.project) if args.project else "",
        exact=bool(getattr(args, "exact", False)),
        platform=args.platform or sys.platform,
        prefix=os.path.abspath(parse_path(args.prefix)) if args.prefix else "",
        environ=dict(os.environ),
        enforce_secure_system=not args.prefix,
        scope=getattr(args, "scope", "all"),
    )

def listing(args: argparse.Namespace) -> dict[str, Any]:
    with WatchPlan() as plan:
        return plan.finish(discovery.collect(environment(args)))

def roots(args: argparse.Namespace) -> dict[str, Any]:
    env = environment(args)
    rows = [
        {
            "agent": entry.agent,
            "kind": entry.kind,
            "path": NativePath(path),
            "precedence": entry.precedence,
            "exists": os.path.isdir(path),
            "doc": entry.doc,
        }
        for entry, path in discovery.resolve_roots(env)
    ]
    return {"ok": True, "schemaVersion": 1, "count": len(rows), "roots": rows}

def apply(args: argparse.Namespace) -> dict[str, Any]:
    return applying.apply(environment(args), args.id, list(args.agent), args.state)

def agents(_: argparse.Namespace) -> dict[str, Any]:
    return {
        "ok": True,
        "schemaVersion": 1,
        "agents": [
            {"id": key, "name": label, "doc": registry.DOCS.get(key, "")}
            for key, label in registry.AGENT_LABELS.items()
        ],
    }

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="agent-skillsctl")
    commands = parser.add_subparsers(dest="command", required=True)
    for name, handler, helptext in (
        ("list", listing, "List discovered agent skills"),
        ("roots", roots, "List every documented root and whether it exists"),
        ("agents", agents, "List supported agents and their documentation"),
        ("apply", apply, "Link or unlink one skill for one or more agents"),
    ):
        command = commands.add_parser(name, help=helptext)
        command.add_argument("--project", default="")
        command.add_argument("--exact", action="store_true")
        command.add_argument("--home", default="")
        command.add_argument("--prefix", default="")
        command.add_argument("--platform", default="")
        command.add_argument("--json", action="store_true")
        command.set_defaults(handler=handler)
        if name == "list":
            command.add_argument("--scope", choices=SCOPES, default="all")
        if name == "apply":
            command.add_argument("--id", required=True)
            command.add_argument("--agent", action="append", required=True)
            command.add_argument("--state", choices=("on", "off"), required=True)
    return parser

def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    payload = args.handler(args)
    emit(payload)
    return 0 if payload.get("ok", True) else 1
