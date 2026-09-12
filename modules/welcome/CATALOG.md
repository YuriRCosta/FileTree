# Welcome catalog

Welcome ships local core entries and help. It makes no registry request.
`Module.acceptCatalog(text)` accepts an optional versioned JSON document;
unavailable or rejected input leaves the current local/last-valid catalog intact.
The input is bounded to 65,536 JavaScript string units and 128 entries.

Version 1 has `version: 1` and `entries: []`. Each entry declares:

| Field | Meaning |
| --- | --- |
| `id` | Unique module identity using the existing module ID syntax, at most 128 characters |
| `name` | Display name, at most 64 characters |
| `source` | HTTPS source link, at most 2,048 characters; opened only on explicit activation |
| `hostContract` | Required existing FileBlade host contract, positive integer |
| `lifecycle` | Advertised `on-demand` or `persistent` activation policy |

Compatibility compares `hostContract` with the injected module context's current
contract version. Lifecycle describes the advertised extension; it is not an
installation or permission claim. Catalog metadata never registers a module,
starts a service, loads QML or installs anything. Unrecognized fields are dropped.
Malformed, duplicate, oversized and newer-schema catalogs are rejected as a whole.

Installed extensions continue through the existing registry and blade picker;
built-ins remain available without this catalog. A future registry transport or
installation action requires its own explicit contract and verification. Welcome
has no such transport or installation path in this version.
