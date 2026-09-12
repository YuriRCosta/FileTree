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

The cumulative runtime at 405c9f8 still requires FILEBLADE_SPIKE_HOME and
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
defaults are untouched. Removal and role reversal are a separate delivery
task, not implemented by these commands yet.

The stable launcher resolves its physical runtime once under a shared
installation lock. Installation and rollback require the exclusive lock and
refuse while a launched session retains the shared lock. This is a safe
busy refusal, not graceful shutdown or a claim that dirty Notes can already
be flushed. The actual Quickshell/authority descriptor lifetime still needs
native qualification. A runtime launched directly outside the stable launcher
does not participate in this delivery lock and must not be updated this way.

Package-owned conventional executable paths and unrelated launchers are
refused. Modified owned launchers, invalid receipts and receipt/pointer
disagreement are also refused. Full package mapping and external package
removal recovery remain task 9.3/9.4 work. Current payload and installer
dependency contracts must match; a contract-changing upgrade needs explicit
compatibility work before it can be accepted.

`tests/vm/expectations/91-delivery-install.sh PAYLOAD` checks collision
preservation, repeated install, distinct activation/rollback, busy refusal,
shared-lock status, settings/Notes preservation, and real process-group kills
during copy and immediately before/after the activation rename. Its alternate
payload manifests differ only in legal JSON whitespace, exercising distinct
transaction identities without inventing another backend version. Test-only
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
dependencies from the verified payload. Its two local sources are hashed;
there is no download step. The output contains the package archive,
PKGBUILD and its local inputs. Build helpers are not added to the runtime.

The `fileblade-native` package owns the unchanged payload beneath
`/usr/lib/fileblade`, a thin `/usr/bin/fileblade` launcher, a
`/usr/bin/fileblade-bin` backend link and a standard license link. Stripping
and debug splitting are disabled. The builder extracts the resulting package
and verifies its inner payload again before publishing the output.
The payload root is normalized to mode 755, as in direct installation, so
a private input directory does not become a root-only installed runtime.

There is no package install hook. Installing a package selects no desktop
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
