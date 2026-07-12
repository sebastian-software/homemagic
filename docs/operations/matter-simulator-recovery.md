# Matter Simulator Operations and Recovery

This procedure applies only to deterministic simulator evidence. It does not
commission physical Matter devices and does not prove BLE, Thread, IPv6, mDNS,
CASE, fabric portability, or interoperability with another controller.

## Normal lifecycle

Use the authenticated `/rpc` endpoint for all admission and reads:

1. Call `matter.fabric.create` with a stable idempotency key. Retain the returned
   operation ID and poll `matter.operations.get` until terminal.
2. Call `matter.nodes.commission.start`. Send setup bytes exactly once to
   `matter.nodes.commission.submit` on `/rpc/sensitive` for that operation ID.
3. Use `matter.nodes.list`, `matter.nodes.get`, and `matter.diagnostics.get` for
   bounded state. Never infer success solely from an HTTP response.
4. Use `commands.execute`, not a Matter cluster method, for light, level, cover,
   or lock behavior. Unlock stays validated until an exact user calls
   `matter.unlock.approve` with the command ID.
5. Call `matter.nodes.remove` with a new idempotency key when decommissioning.

An admission response with `execution: durable_pending` is still durable. Retry
the same method with the same key after the worker is available. A different
request with that key returns `matter_conflict`.

## Sensitive exchange

Only `/rpc/sensitive` accepts setup, export delivery, or restore material. Do not
send these values to `/rpc`, WebSocket events, issue trackers, logs, command
arguments, or shell history. The endpoint has three methods:

- `matter.nodes.commission.submit` accepts setup bytes after commissioning
  admission;
- `matter.fabric.export.deliver` performs one-time simulator export delivery;
- `matter.fabric.restore.submit` accepts the simulator envelope and recovery
  bytes after restore admission.

Input is bounded and converted immediately to non-serializable memory. A
`matter_sensitive_timeout` means the transport stopped waiting; it does not
prove that the daemon discarded an accepted request. Query the operation before
retrying. After process loss, HomeMagic never reconstructs or replays sensitive
input. A requested operation that still needs bytes requires explicit
resubmission; an operation past an ambiguous dispatch boundary fails closed.

## Cancellation and restart

Call `matter.commissioning.cancel` with the commissioning operation ID and a new
idempotency key. A commissioning operation still in `requested` is cancelled
locally. Once controller work began, a separate cancellation operation becomes
durable and reconciles both histories atomically.

After restart, inspect `matter.operations.list` and then each nonterminal
operation with `matter.operations.get`:

| Observed phase | Operator action |
| --- | --- |
| `requested` | Retry the same admission key; resubmit sensitive input if required. |
| `creating_fabric`, `loading_fabric` | Let the daemon reconcile live simulator status; do not create a second operation. |
| `validating_setup` through `subscribing` | Query diagnostics; never invent or replay setup bytes. Cancellation is explicit. |
| `cancelling`, `removing`, `cleaning_secrets` | Let reconciliation prove the outcome; do not repeat controller work manually. |
| `reading_gap`, ambiguous `subscribing` | Expect fail-closed `repair_required`, not redispatch. |
| `failed`, `cancelled`, `repair_required`, `completed` | Terminal. Preserve operation and repair evidence before new work. |

## Subscription repair

Use `matter.diagnostics.get` with an explicit evaluation timestamp. When the
diagnostic remediation is `repair_subscription`, call
`matter.subscriptions.repair` with the exact fabric and node IDs. The workflow
performs at most one bounded gap read and a fixed number of resubscribe attempts.
`waiting` means retry only at or after the persisted deadline. Exhaustion opens
durable repair evidence; increasing client retries does not increase the work
budget.

## Partial removal cleanup

A controller may remove a node while local projection cleanup fails, or vice
versa. HomeMagic retains the node, operation progress, normalized controller
error, and repair record rather than hiding the partial state. Inspect
`matter.nodes.get`, `matter.operations.get`, and `matter.diagnostics.get`.
Resolve the recorded repair action before attempting another removal. Do not
delete SQLite rows, secret references, or simulator checkpoints manually.

## Event reconnect

Subscribe on `/rpc/ws` with `events.subscribe` and the last committed cursor.
Matter operation events contain only operation ID, kind, previous/new phase, and
revision. The server advances past other actors' hidden events. On
`events.lagged`, continue from `last_delivered_cursor`; on cursor expiry, perform
a bounded state resync before subscribing from the earliest retained cursor.
