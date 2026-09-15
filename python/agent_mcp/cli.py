from __future__ import annotations

import argparse
import json
import sys

import agent_usage
from fileblade_inventory import SCOPES, WatchPlan

from .apply import Applier
from .inventory import Inventory, bounded_json
from .model import SCHEMA_VERSION

MAX_RESTORE_PAYLOAD_BYTES = 1024 * 1024


def stdin_payload() -> tuple[str, str]:
    payload = sys.stdin.readline(MAX_RESTORE_PAYLOAD_BYTES + 2)
    if payload.endswith("\n"):
        payload = payload[:-1]
    if not payload:
        return "", "restore payload is missing"
    if len(payload.encode("utf-8")) > MAX_RESTORE_PAYLOAD_BYTES:
        return "", "restore payload exceeds its byte limit"
    return payload, ""


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(prog="agent-mcpctl")
    commands = result.add_subparsers(dest="command", required=True)
    recovery = commands.add_parser("recovery-list", help="List private recovery ids and dates without exposing their contents")
    recovery.add_argument("--json", action="store_true", required=True)
    listing = commands.add_parser("list", help="list configuration-only MCP definitions")
    listing.add_argument("--project", required=True)
    listing.add_argument("--json", action="store_true", required=True)
    listing.add_argument("--watch", action="store_true", help="Include native source directories for the host's private watch transport")
    listing.add_argument("--scope", choices=SCOPES, default="all")
    applying = commands.add_parser("apply", help="write or remove one definition in an agent's user config")
    applying.add_argument("--project", required=True)
    applying.add_argument("--id", required=True)
    applying.add_argument("--agent", action="append", required=True)
    applying.add_argument("--state", choices=("on", "off"), required=True)
    applying.add_argument("--json", action="store_true", required=True)
    for command in ("remove", "prepare-remove", "remove-prepared"):
        removing = commands.add_parser(command, help="prepare or perform removal of a listed definition")
        removing.add_argument("--project", required=True)
        removing.add_argument("--id", required=True)
        removing.add_argument("--transaction-id", default="")
        removing.add_argument("--json", action="store_true", required=True)
        if command == "remove-prepared":
            removing.add_argument("--payload-stdin", action="store_true", required=True)
    for command in ("restore", "discard"):
        restoring = commands.add_parser(command, help="Restore a removal or permanently discard its private recovery record")
        restoring.add_argument("--project", default="")
        restoring.add_argument("--record-id", required=True)
        restoring.add_argument("--payload-stdin", action="store_true")
        restoring.add_argument("--json", action="store_true", required=True)
    return result


def attach_usage(document: dict) -> None:
    """Counts belong to a definition only when its name resolves to one row.

    A transcript records the server name the agent called, not which
    configuration supplied it, so a name held by two definitions is reported
    on neither.
    """
    definitions = document.get("definitions")
    if not isinstance(definitions, list) or not definitions:
        return
    owners: dict[str, int] = {}
    for item in definitions:
        if isinstance(item, dict):
            owners[str(item.get("name") or "")] = owners.get(str(item.get("name") or ""), 0) + 1
    usage = agent_usage.collect(known=None)
    ambiguous = 0
    for item in definitions:
        if not isinstance(item, dict):
            continue
        name = str(item.get("name") or "")
        counts = None
        for recorded, values in usage["servers"].items():
            if name in agent_usage.server_aliases(recorded):
                counts = values
                break
        if counts is None:
            item.update(agent_usage.Tally().document())
            continue
        if owners.get(name, 0) > 1:
            ambiguous += 1
            item.update(agent_usage.Tally().document())
            item["usageAmbiguous"] = True
            continue
        item.update(counts)
        metrics = item.get("metrics")
        if not isinstance(metrics, dict):
            metrics = {}
            item["metrics"] = metrics
        metrics.update(counts)
    document["usageTranscripts"] = usage["transcripts"]
    document["usageUnreadable"] = usage["unreadable"]
    if ambiguous:
        document["usageAmbiguous"] = ambiguous


def main(arguments: list[str] | None = None) -> int:
    options = parser().parse_args(arguments)
    if options.command == "recovery-list":
        document = Applier(Inventory("")).recovery.inventory()
        sys.stdout.write(json.dumps(document, separators=(",", ":")) + "\n")
        return 0 if document["ok"] else 1
    if options.command == "list":
        try:
            if options.watch:
                with WatchPlan() as plan:
                    document = plan.finish(Inventory(options.project, scope=options.scope).scan())
            else:
                document = Inventory(options.project, scope=options.scope).scan()
            attach_usage(document)
            output = bounded_json(document)
        except (OSError, ValueError, TimeoutError):
            sys.stdout.write('{"ok":false,"error":"Inventory failed, not probed","definitions":[],"healthBasis":"configuration-only","schemaVersion":1,"truncated":true,"warnings":[{"code":"inventory-failed","sourceId":"inventory"}]}\n')
            return 1
        sys.stdout.write(output + "\n")
        return 0
    if options.command in ("apply", "remove", "prepare-remove", "remove-prepared", "restore", "discard"):
        try:
            applier = Applier(Inventory(options.project))
            if options.command == "apply":
                document = applier.apply(options.id, options.agent, options.state)
            elif options.command in ("remove", "prepare-remove"):
                document = applier.remove(options.id, prepare=options.command == "prepare-remove", transaction_id=options.transaction_id)
            elif options.command == "remove-prepared":
                raw_payload, payload_error = stdin_payload()
                expected = json.loads(raw_payload) if not payload_error else None
                document = applier.remove(options.id, expected_payload=expected, transaction_id=options.transaction_id) if isinstance(expected, dict) else applier.failure("prepared recovery payload is missing or invalid")
            elif not options.payload_stdin:
                document = applier.recovery.discard_payload(options.record_id) if options.command == "discard" else applier.restore(options.record_id, None)
            else:
                raw_payload, payload_error = stdin_payload()
                document = applier.failure(payload_error) if payload_error else (applier.recovery.discard_payload(options.record_id, raw_payload) if options.command == "discard" else applier.restore(options.record_id, raw_payload))
        except (OSError, ValueError, TimeoutError):
            document = {
                "ok": False,
                "schemaVersion": SCHEMA_VERSION,
                "project": "<project>",
                "changed": False,
                "message": "apply failed",
                "results": [],
            }
        sys.stdout.write(json.dumps(document, ensure_ascii=True, separators=(",", ":"), sort_keys=True) + "\n")
        return 0 if document["ok"] else 1
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
