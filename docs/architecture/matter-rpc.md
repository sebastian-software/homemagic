# Matter RPC Architecture

## Composition

`MatterApiServices` is an explicit cloneable bundle of the fabric workflow,
administration, node inventory, and diagnostics application services.
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

## Stable errors

| JSON-RPC code | Stable data code | Meaning |
| --- | --- | --- |
| `-32602` | `invalid_matter_params` | malformed, unknown, zero, or oversized params |
| `-32603` | `matter_internal` | durable state failed without transport detail leakage |
| `-32060` | `matter_unavailable` | router has no Matter service bundle |
| `-32063` | `matter_denied` | current durable actor/grant does not authorize the read |
| `-32064` | `matter_not_found` | resource is missing or belongs to another actor |
| `-32065` | `matter_controller_unavailable` | bounded controller status failed |
| `-32066` | `matter_evidence_mismatch` | simulator-only evidence boundary was violated |

Adapter text, repository errors, secrets, and foreign identifiers never enter
error data. The SQLite-backed contract covers empty and populated reads, strict
params, bounds, denied and foreign actors, unavailable composition, schema
canaries, and byte-stable operation visibility after repository reopen.
