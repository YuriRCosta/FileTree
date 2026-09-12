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

## Lazy lifecycle

`run-lifecycle.py prepare` creates an isolated declaration fixture and backs up
the guest Service before adding a persistent test probe. It explicitly selects
the freshly built `target/release/fileblade` for the Service backend. Restart C,
verify the populated catalog and inspect an open blade before running
`python3 -B tests/core_modules/run-lifecycle.py check` in the staged checkout.
Do not change staged files during the check: the plugin watcher reloads Service.

The check opens and closes each real core view twice, waits for both inventory
watches, then requires zero observers, scans, watches and pending subscription
callbacks. Backend thread count must return to the closed-view baseline, with
no child process left. Hook/MCP declarations contain an execution sentinel;
viewing them must leave it absent, and MCP credentials must remain redacted.
The check restores its original root and layout in `finally`. Results remain
in `/tmp/rivet-core-lifecycle/results.json` inside C. Run
`python3 -B tests/core_modules/run-lifecycle.py restore`, then restart and inspect
the plain Service. Only the temporary Service source is instrumented.

`tests/qml/tst_core_lifecycle.qml` uses all four actual providers with the shared
ArtifactInventory. Its controlled backend checks two-view sharing, final detach,
late callbacks and accepted mutation arguments after detach/project change.
The existing inventory already supplies this lifecycle; no parallel provider
manager or worker implementation is needed. The inactive provider retains its
bounded inventory rows for reopening; unused scans and subscriptions stop.
This qualifies the shared/plugin view lifecycle, not native authority continuation
or a complete idle-performance comparison.

## Settings version changes

`settings.py` runs only from the plugin staged in the allocated guest. Build
and push the current binary first. Its `prepare` phase backs up Service,
StateController, state and layout under `/tmp/rivet-settings-version`, selects
the fresh binary, and attaches a temporary IPC probe to the real Service.
Restart the shell and check modules plus an open-blade screenshot before each
following qualification phase:

```sh
python3 -B tests/core_modules/settings.py prepare
```

After restarting, run `settings.py seed`. It saves a choice equal to the Git
default while leaving property icons untouched, through the real state writer.
Run `settings.py revise`, restart and inspect again, then `settings.py verify`.
The guest-only revised defaults must change the untouched icons preference
while retaining the explicit Git choice and unknown fields. The same check
exercises linked-column, search and sort markers and reset-to-defaults.

Stop the guest shell before `settings.py restore` so no queued write can
replace the restored documents. Restart once more, inspect the original
layout, and retain the logs and screenshots. The backup directory remains
available for recovery; archive or remove it before a new run. This qualifies
state/settings evolution in the plugin Service; desktop binding-role receipts
are a separate runtime integration contract.
