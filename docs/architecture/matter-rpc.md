# Matter RPC Architecture

## Composition

`MatterApiServices` is an explicit cloneable bundle of the fabric workflow,
administration, node inventory, diagnostics, optional mutation workflows,
daemon execution handoff, and governed common command service.
`router_with_matter` installs that bundle in per-router state; no transport code
constructs repositories, controllers, policy, or global singletons.

Bearer authentication runs before dispatch. The resulting durable actor is the
only actor passed to application services. All Matter params use
`deny_unknown_fields`, so attempts to add `actor_id`, grant, policy, controller,
cluster, attribute, or command context fail as `invalid_matter_params`.

## Read methods

| Method | Required params | Bound | Result schema |
| --- | --- | --- | --- |
| `matter.fabric.status` | none | one installation fabric/status read | `matter.fabric.status.v1` |
| `matter.operations.list` | none | `limit`, default 50, maximum 256 | `matter.operations.v1` |
| `matter.operations.get` | `operation_id` | one actor-owned operation | `matter.operation.v1` |
| `matter.nodes.list` | `fabric_id` | `limit`, default 50, maximum 256 | `matter.nodes.v1` |
| `matter.nodes.get` | `fabric_id`, `node_id` | one installation/fabric-scoped node | `matter.node.v1` |
| `matter.diagnostics.get` | `evaluated_at` | `limit`, default 50, maximum 256 | `matter.diagnostics.rpc.v1` |

The committed machine-readable params and envelope catalog is
[`matter-rpc-reads-v1.json`](../api/schemas/matter-rpc-reads-v1.json). The API
crate embeds and parses that exact file through `MATTER_READ_RPC_SCHEMA_V1`, so
documentation and executable schema cannot drift independently.

Fabric output maps only secret-reference-free metadata and normalized
controller status. Operations are already filtered by immutable actor binding.
Node reads retain bounded SDK-neutral descriptor metadata but expose no invoke
or raw-write method. Diagnostics use the separately proven read-only service and
an explicit evaluation timestamp.

## Mutation admission

| Method | Required params | Immediate result | Execution |
| --- | --- | --- | --- |
| `matter.fabric.create` | `idempotency_key` | `matter.operation.v1` | daemon queue |
| `matter.nodes.commission.start` | `idempotency_key` | `matter.operation.v1` | waits for sensitive setup |
| `matter.commissioning.cancel` | `operation_id`, `idempotency_key` | `matter.operation.v1` | local cancel or daemon queue |
| `matter.nodes.remove` | `node_id`, `idempotency_key` | `matter.operation.v1` | daemon queue |
| `matter.subscriptions.repair` | `fabric_id`, `node_id`, `idempotency_key` | `matter.operation.v1` | daemon queue |
| `matter.fabric.export.start` | `idempotency_key` | `matter.operation.v1` | waits for sensitive delivery |
| `matter.fabric.restore.start` | `idempotency_key` | `matter.operation.v1` | waits for sensitive input |
| `matter.unlock.approve` | `command_id` | `matter.unlock.approval.v1` | governed common command service |

Admission commits the actor binding, idempotency key, canonical secret-free
request hash, operation, and progress before returning. New and equivalent
requests return the durable operation. Reusing a key for another request is a
stable conflict. Queue saturation cannot erase admission: the envelope reports
`durable_pending`, allowing a daemon restart or an equivalent retry to wake it
again.

`MatterExecutionHandle` is a bounded channel only. It cannot execute a workflow.
`MatterExecutionWorker` owns the receiver and application workflow services;
the daemon controls its loop and shutdown. HTTP handlers neither spawn nor own
operation tasks. This keeps controller work independent from HTTP disconnects.

## Sensitive exchange

The untraced `/rpc/sensitive` route exposes exactly three methods:

| Method | Sensitive value | Bound |
| --- | --- | --- |
| `matter.nodes.commission.submit` | setup bytes | 1–1,024 bytes |
| `matter.fabric.export.deliver` | one-time export result | one operation |
| `matter.fabric.restore.submit` | envelope and recovery key | 1–1,048,576 and 1–1,024 bytes |

No ordinary route recognizes these methods, and the sensitive route rejects
every ordinary method. Request DTOs have no `Debug` representation. Byte arrays
are bounded and converted immediately to non-serializable `SecretValue` inputs.
The worker receives them through a bounded in-memory request/reply message. A
transport timeout does not cancel an already accepted daemon request, and a
restart never guesses or replays lost sensitive input.

The committed executable catalog is
[`matter-rpc-mutations-v1.json`](../api/schemas/matter-rpc-mutations-v1.json).
Its ordinary method definitions contain no setup, envelope, recovery, actor,
policy, cluster, attribute, or command escape-hatch input. Export bytes are
returned only by the sensitive endpoint and never enter ordinary operation
state.

## Stable errors

| JSON-RPC code | Stable data code | Meaning |
| --- | --- | --- |
| `-32602` | `invalid_matter_params` | malformed, unknown, zero, or oversized params |
| `-32603` | `matter_internal` | durable state failed without transport detail leakage |
| `-32060` | `matter_unavailable` | router has no Matter service bundle |
| `-32061` | `matter_conflict` | idempotency or durable operation state conflicts |
| `-32062` | `matter_sensitive_timeout` | bounded sensitive exchange wait elapsed |
| `-32063` | `matter_denied` | current durable actor/grant does not authorize the read |
| `-32064` | `matter_not_found` | resource is missing or belongs to another actor |
| `-32065` | `matter_controller_unavailable` | bounded controller status failed |
| `-32066` | `matter_evidence_mismatch` | simulator-only evidence boundary was violated |

Adapter text, repository errors, secrets, and foreign identifiers never enter
error data. The SQLite-backed contract covers empty and populated reads, strict
params, bounds, denied and foreign actors, unavailable composition, schema
canaries, and byte-stable operation visibility after repository reopen.
