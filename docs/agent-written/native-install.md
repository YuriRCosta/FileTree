This file was written by an agent.

# Local native payloads

`tools/native` builds an inventoried runtime directory from a committed
source tree, a matching prebuilt backend and that target's notices. It does
not acquire software, build dependencies or enable desktop roles.

```
tools/native stage SOURCE BACKEND TARGET NOTICES OUTPUT
tools/native verify OUTPUT
tools/native check OUTPUT
```

Supported target identifiers are `x86_64-unknown-linux-gnu`,
`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-gnu` and
`aarch64-unknown-linux-musl`. GNU inputs must name the corresponding glibc
interpreter; musl inputs must be static. `verify` checks the whole file set,
SHA-256, modes, required files and ELF architecture/ABI without executing the
backend. `check` additionally requires the matching machine, runtime packages
and a runnable backend with the declared version. Runtime package checks
currently qualify Arch/Omarchy only. Installed packages do not prove a
working portal session or graphical runtime.

`packaging/runtime.json` declares source roots, required files, executable
dependencies and the tested package floor. `payload.json` records the source
commit and target plus every shipped file's relative path, digest and mode.
The manifest itself is outside its own digest set. Inventories reject links,
special files and unsupported names/modes. Runtime directories use mode 755;
files use 644 or 755. Missing files and extra files both fail validation.
The source tree must have no tracked changes during staging. Untracked files
are not runtime inputs. Output publication uses a temporary sibling
directory and refuses to replace an existing destination.

The producer is responsible for building the supplied backend and notices
from the named source/target. A digest detects corruption; it is not a signed
build attestation. These local payloads carry no remote provenance claim.
Cross-target verification is not an ARM runtime or physical-hardware pass.

The current runtime launcher expects its backend at
`target/release/fileblade`; the payload retains that path without requiring
Cargo or a source checkout at launch. All QML relative imports and core Python
helpers are preserved. The app-private adapter and its upstream license are
included. Developer-only app/ovm-spike and app/qualification helpers are
excluded. Runtime-owned service/portal metadata, when committed beneath app/,
is included; packaging does not invent or enable a chooser implementation.

The qualified cumulative runtime at ffaa351 still requires FILEBLADE_SPIKE_HOME and
uses fixture-only write authority. Staging its complete file set does not
make that launcher a production startup route. Until runtime replaces that
contract, launch qualification must use an explicitly isolated guest fixture
and must not be called host install readiness.

Run `tests/vm/expectations/90-delivery-payload.sh SOURCE BACKEND TARGET NOTICES`
inside the assigned Omarchy guest for integrity, malformed-inventory,
wrong-architecture and missing-dependency checks. The fresh staged app launch,
module catalog and screenshot are separate live evidence; this script does
not claim them.

## User-local installation

```
PAYLOAD/tools/native install PAYLOAD
PAYLOAD/tools/native status
PAYLOAD/tools/native rollback
PAYLOAD/tools/native remove
```

The installer is self-contained in the payload. It stores runtime versions
under `$XDG_DATA_HOME/fileblade/installation/versions`, falling back to
`~/.local/share`, and creates `~/.local/bin/fileblade`. The receipt is
`installation/active/receipt.json`. A payload manifest's SHA-256 names its
version directory, so successive development builds with the same version
number remain distinct and recoverable. No setting or Note is stored there.

Installation validates the input, copies it to a temporary sibling, validates
the copy and publishes it. A generation pairs the receipt with a symlink to
its runtime. One atomic pointer switch activates that pair. The receipt
records the previous payload, and rollback performs the same activation in
reverse. Existing runtime generations and interrupted staging directories
are retained; no automatic pruning is implemented. Settings and desktop
defaults are untouched. Removal preserves generation receipts and user data.
Role reversal uses runtime's published maintenance interface.

The stable launcher resolves its physical runtime once under a shared
installation lock. Maintenance verifies the active payload under a shared
lock, calls its lifecycle CLI, then takes the exclusive lock and rechecks
the activation pointer. A changed pointer requires retry. Update and rollback
call `app/launch native drain --timeout-ms 30000 --json`; removal first calls
`app/launch native roles disable --all --json`. Nonzero exits or unknown,
malformed or incomplete success results preserve the runtime. A failed drain
after reversal can leave roles disabled; the diagnostic reports that outcome.
The current spike does not implement these entry points and therefore refuses
maintenance. Closing its view alone leaves the authority holding the lock.
A runtime launched directly outside the stable launcher
does not participate in this delivery lock and must not be updated this way.

Package-owned conventional executable paths and unrelated launchers are
refused. Modified owned launchers, invalid receipts and receipt/pointer
disagreement are also refused. Arch package mapping is described below;
external package removal recovery remains task 9.4 work. Current payload and installer
dependency contracts must match; a contract-changing upgrade needs explicit
compatibility work before it can be accepted.

`tests/vm/expectations/91-delivery-install.sh PAYLOAD` checks collision
preservation, repeated install, distinct activation/rollback, busy refusal,
shared-lock status, settings/Notes preservation, and real process-group kills
during copy and immediately before/after the activation rename. Its alternate
transaction payload manifests differ only in legal JSON whitespace. A separate
case changes a compatible dependency floor and verifies that the prior
contract prevents activation while preserving rollback. Test-only
command wrappers inject interruptions; production code has no fault hooks.
These are process-interruption checks, not physical power-loss tests.

## Arch package from the same payload

```
packaging/build PAYLOAD OUTPUT_DIRECTORY
```

Run this maintainer helper on the payload's architecture with the existing
Arch `makepkg`, `fakeroot` and `bsdtar` tools. It accepts stable versions,
requires an absent output directory, and installs no build dependencies.
The generated PKGBUILD takes its version, architecture and package
dependencies from the verified payload. Its three local sources are hashed;
there is no download step. The output contains the package archive,
PKGBUILD and its local inputs. Build helpers are not added to the runtime.

The `fileblade-native` package owns the unchanged payload beneath
`/usr/lib/fileblade`, a thin `/usr/bin/fileblade` launcher, a
`/usr/bin/fileblade-bin` backend link and a standard license link. Stripping
and debug splitting are disabled. The builder extracts the resulting package
and verifies its inner payload again before publishing the output.
The payload root is normalized to mode 755, as in direct installation, so
a private input directory does not become a root-only installed runtime.

Installing a package selects no desktop
roles, defaults, bindings or autostart. User-level installations remain
separate; the direct installer detects conventional pacman-owned paths and
refuses updates when they coexist. Package files must be updated or removed
through pacman. A direct launcher may still shadow the package after an
external pacman install; the installer diagnoses that collision and preserves
both trees.

The current launcher still has the documented spike-only startup contract.
Package mapping does not qualify that startup, provide a chooser, or register
runtime-owned desktop/service metadata that has not yet been implemented.
Mapping those descriptors and checking real companion-mode coexistence remain
part of native integration and task 9.4. An ARM package requires an actual
ARM payload and matching Arch build environment; no ARM execution follows
from the inventory format accepting an ARM target.

Inside the assigned guest,
`tests/vm/expectations/92-delivery-package.sh SOURCE PAYLOAD` builds and
extracts the package, compares its payload/dependencies, installs it through
pacman, verifies ownership, checks direct-update refusal, removes the package
and checks preservation of the direct receipt and personal-default fixtures.
It refuses to replace a preexisting fileblade-native package and cleans up
its own package fixture on failure. Structural fixtures are not a native app
launch or a real chooser/reveal fallback pass.

## Removal and stale activation

`remove` uses the same owner and exclusive-lock checks as installation. It
refuses package ownership, changed launchers and changed inventoried payloads.
After validation it withdraws activation, unlinks its launchers and moves
owned versions into a private discard directory before deleting them.
`installation/removing` retains the receipt during deletion; retry `remove`
if interrupted. Installation refuses while this marker exists. Successful
removal retains `installation/removed/receipt.json` and generation history.
Unknown entries and incomplete staging directories are preserved. No setting,
Note, recovery journal or desktop default is deleted. Repeated removal is
safe, and a later explicit local installation can reuse the installation root.

Rollback validates its recorded previous payload before requesting drain.
Missing/damaged current payloads or missing activation with receipt history
require restoring the verified current runtime/owned pointer first: delivery
cannot prove a surviving authority stopped by calling a different candidate.
Invalid receipts are refused. Recovery never guesses a generation from
timestamps or directory order. Runtime recovery for an unavailable active
maintenance entry point remains unqualified.

The package launcher takes a shared lock on `/usr/share/fileblade-native/lock`
before checking pacman's configured database lock and the held lock inode. Existing launches block
the ALPM idle preflight; new launches refuse throughout a package transaction,
including after that preflight. Unrelated transactions using the same
database also prevent launch until their lock is released. A stale pacman lock requires pacman's normal
recovery; FileBlade does not delete it. Qualification covers the configured
system database, not ad-hoc `--dbpath` or `--config` overrides. The hook edits
no user defaults. Active operations, dirty Notes and enabled-role reversal
still need runtime implementation and live qualification. External stale role
recovery uses runtime's opaque receipt, never the direct receipt.

`tests/vm/expectations/93-delivery-remove.sh PAYLOAD` checks stale activation,
ownership preservation, busy refusal and an actual process-group kill during
runtime deletion followed by retry. E92 additionally holds the package lock
and checks that real pacman removal aborts while preserving installed files.

`tests/vm/expectations/94-delivery-lifecycle.sh SOURCE PAYLOAD` creates an
explicit maintenance fixture, checks failure/result handling, the active
launcher path, shared-lock ordering and activation identity changes, then runs
E91/E93 with it. These are caller/transaction checks; they do not implement or
qualify runtime draining or desktop-role reversal.

## Installed expectation adapter

Select the adapter and the native shape for installed expectations:

```bash
export OVM="$PWD/tests/vm/native-ovm"
export OVM_REAL="$HOME/.claude/skills/test-omarchy-plugin/scripts/ovm"
export SKIP_PUSH=1 FILEBLADE_SHAPE=native
"$OVM" ipc data-goblin.fileblade status
```

Keep the assigned `OVM_HOME` and `OVM_SSH_PORT`. The adapter resolves the
installed stable user launcher, or the packaged launcher when no user launcher
exists. `FILEBLADE_NATIVE_LAUNCHER` can name an explicit guest stable launcher.
It checks the direct activation receipt and manifest identity; installation
and qualification own the full payload inventory check. It exports the
published native payload/backend/state environment for native commands.

`ipc` routes FileBlade targets through `native ipc --` on the stable launcher.
Other targets, `ssh` command strings and other ovm verbs forward unchanged.
`restart` and `restart-shell` drain first, restart through the stable launcher
and wait for the installed view and eight built-ins. A refused or incomplete
drain leaves activation intact. Launch output is retained in
`$FILEBLADE_NATIVE_STATE_ROOT/native-ovm.log` in the guest.

Push is refused by default. To explicitly install a previously built payload
whose manifest source matches the given tree's HEAD:

```bash
FILEBLADE_NATIVE_PAYLOAD=/absolute/path/to/payload SKIP_PUSH=0 "$OVM" push "$PWD"
```

Push transfers that payload into a temporary guest directory, invokes its
`tools/native install` and removes the temporary copy. Build the payload using
the staging command above; the adapter does not choose a backend or notices.
The shared runner also accepts a relative OVM path. Runtime owns native
control and stop/restart selection in the shared helpers; wheel owns its
remaining plugin-specific callers. Production launcher/drain qualification
and the four backend IPC commands remain pending runtime. SSH strings
forward unchanged; shape selection belongs in their callers.
