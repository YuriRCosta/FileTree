# Core Service integration

Run only in the assigned Omarchy test guest. Build the runtime before staging,
push the checkout with the harness, and restart the shell. Confirm
`omarchy-shell data-goblin.fileblade bladeModules` lists all four built-ins.

The fixture generator adds a test-only Loader to each guest core module after
backing up its source. `LiveProbe.qml` receives the real module object and uses
its existing Service, provider, mutation and artifact-bin handlers. It does not
instantiate a substitute Service or inventory. No test hook is present in the
production module files.

From the staged guest checkout:

```sh
python3 -B tests/vm/fixtures/core-live.py prepare
```

Restart the guest shell again, then select the fixture project:

```sh
omarchy-shell data-goblin.fileblade.control setRoot /tmp/rivet-core-live/project
python3 -B tests/core_modules/run-live.py skills
python3 -B tests/core_modules/run-live.py memory
python3 -B tests/core_modules/run-live.py hooks
python3 -B tests/core_modules/run-live.py mcp
```

Each runner requires an empty artifact bin for its module and emits
`CORE_LIVE_PASS <module>` only after inventory, apply/unapply, removal/restore
and close/reopen pass. Skills and Memory additionally verify consent refusal.
Hooks and MCP verify the helper recovery record matches the artifact manifest
and disappears after successful restore. Their Codex apply targets are guest
user configuration files; the runner saves their initial bytes under the
fixture directory and restores them after unapply. If a run fails before that
point, inspect the retained backup and result before restoring the target.

Save the guest's original layout, root and preferences before running. The
scenarios change the right slot and agent-management consent. Restore those
values after collecting evidence. Snapshots and fixture sources remain under
`/tmp/rivet-core-live`; a second prepare refuses to overwrite that directory's
existing project or instrumentation backups.

Always remove instrumentation, including after a failed scenario:

```sh
python3 -B tests/vm/fixtures/core-live.py restore
```

Restart the shell once more, verify all four definitions still resolve, and
capture an open core blade without the probe. The screenshots prove rendering;
IPC and filesystem assertions prove the tested state transitions. These
plugin-shape checks do not qualify the native authority continuation or the
complete worker lifetime contract.
