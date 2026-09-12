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
