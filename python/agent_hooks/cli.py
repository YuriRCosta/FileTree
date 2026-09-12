from __future__ import annotations

import argparse
import sys
from typing import Any

from fileblade_inventory import SCOPES, WatchPlan

from . import discovery
from .safeio import fit_payload

MAX_RESTORE_PAYLOAD_BYTES = 1024 * 1024

def emit(payload: dict[str, Any]) -> None:
    sys.stdout.write(fit_payload(payload))
    sys.stdout.write("\n")
    sys.stdout.flush()

def run_apply(args: argparse.Namespace) -> dict[str, Any]:
    from . import apply
    return apply.apply(args.project, args.id, args.agent, args.state, args.home, exact=args.exact)

def run_recovery_list(args: argparse.Namespace) -> dict[str, Any]:
    from . import apply
    return apply.recovery_store().inventory()

def run_list(args: argparse.Namespace) -> dict[str, Any]:
    if not args.watch:
        return discovery.collect(args.project, args.home, exact=args.exact, scope=args.scope)
    with WatchPlan() as plan:
        return plan.finish(discovery.collect(args.project, args.home, exact=args.exact, scope=args.scope))

def run_remove(args: argparse.Namespace) -> dict[str, Any]:
    from . import apply
    expected = None
    if args.command == "remove-prepared":
        import json
        try:
            expected = json.loads(sys.stdin.readline(MAX_RESTORE_PAYLOAD_BYTES + 2))
        except (ValueError, RecursionError):
            return apply.failure("", "prepared recovery payload is invalid")
        if not isinstance(expected, dict):
            return apply.failure("", "prepared recovery payload is missing")
    return apply.remove(args.project, args.id, args.home, exact=args.exact,
                        prepare=args.command == "prepare-remove", expected_payload=expected, transaction_id=args.transaction_id)

def run_label(args: argparse.Namespace) -> dict[str, Any]:
    import os
    from pathlib import Path
    from . import labels
    from .safeio import expanded
    home = expanded(args.home) if args.home else Path(os.path.expanduser("~"))
    return labels.set_label(home, dict(os.environ), args.digest, args.text)

def run_restore(args: argparse.Namespace) -> dict[str, Any]:
    from . import apply
    if not args.payload_stdin:
        return apply.recovery_store().discard_payload(args.record_id) if args.command == "discard" else apply.restore(args.record_id, None)
    raw_payload = sys.stdin.readline(MAX_RESTORE_PAYLOAD_BYTES + 2)
    if raw_payload.endswith("\n"):
        raw_payload = raw_payload[:-1]
    if not raw_payload:
        return apply.failure("", "restore payload is missing")
    if len(raw_payload.encode("utf-8")) > MAX_RESTORE_PAYLOAD_BYTES:
        return apply.failure("", "restore payload exceeds its byte limit")
    if args.command == "discard":
        return apply.recovery_store().discard_payload(args.record_id, raw_payload)
    return apply.restore(args.record_id, raw_payload)

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="agent-hooksctl",
                                     description="Agent hook inventory and user-scope hook copies")
    commands = parser.add_subparsers(dest="command", required=True)
    recovery = commands.add_parser("recovery-list", help="List private recovery ids and dates without exposing their contents")
    recovery.add_argument("--json", action="store_true")
    recovery.set_defaults(handler=run_recovery_list)
    listing = commands.add_parser("list", help="List discovered hook definitions (writes nothing)")
    listing.add_argument("--project", default="")
    listing.add_argument("--exact", action="store_true")
    listing.add_argument("--home", default="")
    listing.add_argument("--json", action="store_true")
    listing.add_argument("--watch", action="store_true", help="Include native source directories for the host's private watch transport")
    listing.add_argument("--scope", choices=SCOPES, default="all")
    listing.set_defaults(handler=run_list)
    applying = commands.add_parser("apply", help="Copy a listed hook into, or remove it from, an agent's user hook file")
    applying.add_argument("--project", default="")
    applying.add_argument("--exact", action="store_true")
    applying.add_argument("--home", default="")
    applying.add_argument("--id", required=True)
    applying.add_argument("--agent", action="append", required=True)
    applying.add_argument("--state", choices=("on", "off"), required=True)
    applying.add_argument("--json", action="store_true")
    applying.set_defaults(handler=run_apply)
    for command in ("remove", "prepare-remove", "remove-prepared"):
        removing = commands.add_parser(command, help="Prepare or perform removal of a listed hook")
        removing.add_argument("--project", default="")
        removing.add_argument("--exact", action="store_true")
        removing.add_argument("--home", default="")
        removing.add_argument("--id", required=True)
        removing.add_argument("--transaction-id", default="")
        removing.add_argument("--json", action="store_true")
        if command == "remove-prepared":
            removing.add_argument("--payload-stdin", action="store_true", required=True)
        removing.set_defaults(handler=run_remove)
    for command in ("restore", "discard"):
        restoring = commands.add_parser(command, help="Restore a removal or permanently discard its private recovery record")
        restoring.add_argument("--record-id", required=True)
        restoring.add_argument("--payload-stdin", action="store_true")
        restoring.add_argument("--json", action="store_true")
        restoring.set_defaults(handler=run_restore)
    labelling = commands.add_parser("label", help="Name a listed hook in FileBlade's own label store; an empty text clears it")
    labelling.add_argument("--home", default="")
    labelling.add_argument("--digest", required=True)
    labelling.add_argument("--text", required=True)
    labelling.add_argument("--json", action="store_true")
    labelling.set_defaults(handler=run_label)
    return parser

def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    payload = args.handler(args)
    emit(payload)
    return 0 if payload.get("ok", False) else 1
