from __future__ import annotations

import argparse
import json
import sys
from typing import Any

from fileblade_paths import wire
from fileblade_inventory import SCOPES, WatchPlan

from . import apply, discovery

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
        data = document([], False)
        return data if len(data) <= MAX_OUTPUT_BYTES else b'{"ok":false,"schemaVersion":1,"truncated":true,"items":[]}\n'
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

def encoded_results(payload: dict[str, Any]) -> bytes:
    data = serialized(payload)
    if len(data) <= MAX_OUTPUT_BYTES:
        return data
    trimmed = dict(payload)
    trimmed["results"] = []
    trimmed["ok"] = False
    trimmed["message"] = "apply output exceeded the size bound"
    return serialized(trimmed)

def emit(payload: dict[str, Any]) -> None:
    sys.stdout.buffer.write(encoded(payload))
    sys.stdout.buffer.flush()

def emit_results(payload: dict[str, Any]) -> None:
    sys.stdout.buffer.write(encoded_results(payload))
    sys.stdout.buffer.flush()

def add_location_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--project", default="")
    parser.add_argument("--exact", action="store_true")
    parser.add_argument("--home", default="")
    parser.add_argument("--config", default="")
    parser.add_argument("--prefix", default="")
    parser.add_argument("--json", action="store_true")

def run_list(args: argparse.Namespace) -> dict[str, Any]:
    with WatchPlan() as watch:
        return watch.finish(discovery.collect(args.project, args.home, args.config, None, args.prefix, not args.prefix, args.exact,
                                              args.scope))

def run_apply(args: argparse.Namespace) -> dict[str, Any]:
    return apply.apply(args.project, args.id, list(args.agent), args.state, args.home, args.config, None,
                       args.prefix, not args.prefix, args.exact)

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="agent-memoryctl", description="Agent memory discovery and agent linking")
    commands = parser.add_subparsers(dest="command", required=True)
    listing = commands.add_parser("list", help="List discovered memory and instruction files")
    add_location_arguments(listing)
    listing.add_argument("--scope", choices=SCOPES, default="all")
    listing.set_defaults(handler=run_list, emitter=emit)
    linking = commands.add_parser("apply", help="Link or unlink a memory file for one or more agents")
    add_location_arguments(linking)
    linking.add_argument("--id", required=True)
    linking.add_argument("--agent", action="append", required=True)
    linking.add_argument("--state", choices=list(apply.STATES), required=True)
    linking.set_defaults(handler=run_apply, emitter=emit_results)
    return parser

def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    args.emitter(args.handler(args))
    return 0
