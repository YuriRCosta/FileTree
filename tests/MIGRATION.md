# Core migration validation

`migration::prepare(&legacy, &native, &authority)` consumes runtime's root-bound
`lease::Authority`. Its implementation requires the authority API published at
`lane/runtime` commit c09ef5d; core does not select the backend write mode.
Runtime must call it after `acquire_bound`, before recovery or document hydration,
and map Ready to Full and both other outcomes to ReadOnly.

The importer selects state.json, module state, settings.json, keybindings.json,
blades.json, colors.json, hook labels, module configuration, Hooks/MCP recovery
records and the legacy artifact bin. JSON bytes and unknown fields survive;
module storage directory aliases resolve to core names. Conflicting aliases or
native destinations refuse import. Layout IDs remain readable through the core
registry's existing aliases and singleton recovery policy.

The private `migration-020/receipt.json` under the native state root is a versioned
snapshot of original bytes, directory entries, stored link targets and root
identities. Publication never overwrites a different destination. `copied.json`
certifies that destination publication finished before legacy artifact removal;
`complete.json` certifies the completed move. These checkpoint documents match
the receipt exactly. Retry verifies unfinished sources and completed steps.
A completed import does not overwrite subsequent native edits. Legacy state and
configuration remain available; removed artifact-bin data also remains in the
receipt. There is no automatic rollback CLI in this slice.

The importer does not alter shell activation, desktop bindings, companion
checkouts, generated desktop snippets, journal replay or unrelated shell plugins.
Active and unknown legacy-writer evidence refuse writable ownership.

Run the product tests only in harness C. Compile the test executables with
`cargo test --release --locked --no-run --test migration_prepare --test migration_writer --test migration_documents --test migration_preservation`.
Select the fresh runtime using FILEBLADE_BINARY wherever a suite launches it.
Copy the migration_prepare executable to target/release/rivet-migration-prepare
before the harness push, then restart and verify the Service module catalog and
an open-blade screenshot. In C run that executable and
`python3 -B tests/vm/fixtures/run-migration.py`. The latter generates real
legacy-route Hooks/MCP records using the selected backend; shell/IPC detection
is isolated with deterministic executable fixtures, never host activation.

`tst_migration_state.qml` checks unknown state fields across repeated writes;
`migration_preservation` checks keybindings metadata updates preserve unknown
fields and refuse newer/malformed originals. The importer suite checks first
and repeated launch, resumable publication, conflicting edits, malformed/newer
schema, missing/mismatched helper evidence, module alias conflicts, stored
symbolic links and authority-root replacement. Keep the combined source revision
and any core overlay in the lane milestone; it is not a standalone core gate
until the runtime dependency has been integrated.

The 5.5 consumer acquires both the legacy journal's shared OFD/flock locks and
the artifact bin's exclusive mutation lock after the initial stopped/absent
proof. It repeats the proof while holding them, retains them through publication
and cleanup, and checks root/lock identity before every mutation boundary.
Contention and changed activation evidence yield read-only startup. The probe
uses direct no-follow legacy reads even when native persistence is not registered.
Native documents are also preflighted on repeated startup, so a completed receipt
does not authorize downgrading newer state. The authority stays read-only until
runtime explicitly accepts the preparation outcome. Tests include held locks,
lock replacement, changed evidence, native startup environment and shared roots.
