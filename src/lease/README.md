This file was written by an agent.

# Native authority contract

`Authority::acquire(state_root)` canonicalizes an absolute root, holds its
open directory identity and takes a Linux OFD write lock on `authority.lock`.
The guard outlives recovery, admission, workers and shutdown cleanup. It never
unlinks the lock file. Holder JSON is diagnostic; stale PID text does not
prevent reacquisition after the kernel releases the lock. Unsafe ownership,
permissions, symbolic links and multiple links are refused.

The native launcher sets `FILEBLADE_NATIVE_STATE_ROOT` to the selected
`XDG_STATE_HOME/omarchy/fileblade` namespace and `FILEBLADE_APP_ROOT` to the
runtime payload. This spike still isolates all three XDG roots. It does not
perform core's migration or prevent an old unmodified plugin from reopening
its separate legacy namespace.

`fileblade serve --native-authority` acquires before recovery, then exposes
`authority.sock` with mode 0600. `serve --native-probe` checks readiness.
Ordinary `serve` with that environment is a view relay; it does not own the
lease or apply parent-death termination to the authority. The launcher starts
the authority independently before loading QML. SIGTERM stops admission and
drains accepted work before releasing the guard. View EOF cancels reads and
subscriptions only.

QML identifies a view with `view:true` in its hello. The authority counts
these connections separately from probes, operation queries and workers.
Losing the last view restores owned window borders before waiting for its
accepted work. Terminal operations also restore borders when no view remains,
covering a dim request that finishes after its view detached.

Both transports use protocol v1 and the existing request/response shape.
A native hello additionally reports `authority:true`. Native mutations emit
an acceptance frame before dispatch:

```json
{"v":1,"type":"accepted","id":"view-request","generation":1,"op":"opaque-operation-id","ok":true}
```

Progress and terminal frames carry that same `op`. Their sink retains the
latest progress and complete terminal payload before attempting delivery;
failed subscribers detach without changing the operation cancellation token.
The terminal payload preserves the backend's mappings, partial output,
cancellation and journal warnings. No request arguments are replayed.

A new connection can query results after the accepting view disappears:

```json
{"v":1,"type":"operation","id":"query","generation":2,"action":"get","op":"opaque-operation-id"}
{"v":1,"type":"cancel","op":"opaque-operation-id"}
```

Operation actions are `get`, `fetch`, `subscribe`, and `list`. `get` returns
`{op,complete,progress,result}`; `list` returns operation IDs and completion
flags, including work whose acceptance acknowledgement was lost. `subscribe`
also attaches the connection to subsequent original operation frames.
`fetch` removes a completed entry only after a successful response write;
a failed or oversized response leaves it retained. Pending entries cannot be
fetched away. Cancel by the old view request ID cannot cancel admitted native
work; explicit `cancel` by `op` can.

Completed results remain in the authority for 24 hours or until fetched.
There are at most 256 retained/admitted operations, a configured active-work
limit, 32 sessions, and 32 subscribers per operation. Admission also reserves
64 MiB per active operation against a 1 GiB serialized-result budget.
Socket delivery times out after 200 ms; readiness handshakes after two seconds.
The existing 32 MiB response limit applies to result retrieval. Oversized
results are retained and reported as too large, never silently truncated.
The result cache is in memory; process-death recovery relies on durable
journals, not a persisted terminal-frame cache.

Native direct mutating `_backend`, companion mutation, direct public backend
mutation, preference writes and list-file writes fail with owner-unavailable.
Their existing isolated legacy routes remain available. The native mutating
classification adds persistence and clipboard commands missing from the
legacy classification; operations owns the final shared classification.

The relay deliberately uses buffered read/write loops. On the qualified VM,
Rust's Linux `io::copy` splice optimization held a pipe lock while waiting for
socket input; Qt then blocked in FIONREAD. `tests/native_authority.rs` covers
that idle-pipe boundary, as well as contention, unsafe lock storage, holder
death, EOF, subscriber loss, result fetching and explicit cancellation.

BackendClient exposes `operationAccepted(requestId, generation, operationId)`
and `operationUpdated(operationId, frame)` for native consumers. Its
`operation` method queries or attaches by operation ID. A terminal response
successfully handled by the accepting QML view is acknowledged with an explicit
fetch; this releases ordinary delivered results without filling retention
capacity during long sessions. If the view disappears before handling the
terminal response, it sends no fetch and the result stays available.

The live `40-native-authority.sh` case drives QML selection and admission for
a two-file move across filesystems, observes it running, quits the real native
view, verifies completion and both mappings, relaunches the view, and fetches
the retained result. It requires the staged spike and harness A.
