# Matter capability projection and subscription recovery

Status: implemented for the deterministic light and lock fixtures in E4-005.

## Boundary

Matter descriptors are adapter input, not a public API. The application maps
them through explicit versioned rules and emits only common HomeMagic capability
descriptors and observations. RPC, MCP, automations, and future generated UIs do
not receive cluster invoke or raw write operations.

The initial enabled rules are deliberately narrow:

| Rule | Required endpoint evidence | Common capability |
| --- | --- | --- |
| `on_off` v1 | On/Off Light or Dimmable Light device type, On/Off server role, `OnOff` attribute, Off and On commands | `on_off.v1` |
| `access_control` v1 | Door Lock device type, Door Lock server role, `LockState` attribute, Lock Door and Unlock Door commands | `access_control.v1` |

Level Control and Window Covering rules are declared but disabled. Cluster
presence, even with features and commands, cannot activate `level.v1` or
`position.v1` until later fixtures prove their semantics and safety constraints.

Unmapped server and client clusters become bounded diagnostics under a
`matter.diagnostics.v1.endpoint.*.cluster.*` namespace. Diagnostics retain only
role, revision, feature map, available attribute IDs, and accepted command IDs.
They are read-only evidence and do not expose an invoke function.

## Identity and invalidation

Identity inputs follow ADR-0034:

```text
integration = installation + "matter" + HomeMagic fabric ID
device      = integration + fabric-scoped Matter node ID
endpoint    = device + Matter endpoint number
projection  = fabric + node + endpoint + rule key + rule version
```

Labels, aliases, rooms, addresses, sessions, subscription handles, and
descriptor revisions are excluded from identity. Rediscovery therefore
recreates the same IDs after restart.

The persisted `projection_revision` records the descriptor revision used to
form command assumptions. Any descriptor revision, report path, cluster
revision, feature map, support, or capability-contract change invalidates the
cached projection. A command path must use a newly projected row before it can
dispatch again.

## Report acceptance

Each report is checked against exactly one projected attribute path. Accepted
reports retain:

- adapter-normalized report sequence;
- optional Matter data version, including wrapping comparison;
- source observation time and local receive time;
- notification or bounded gap-read provenance;
- optional common causation and desired-state revision.

Older sequences and backward data versions are rejected. An equal sequence with
equal content is idempotent; an equal sequence with different content is a
conflict. A report marks state fresh only after acceptance. It confirms a
desired revision only when the value matches and causation names that exact
revision; an unbound matching value remains pending.

## Subscription recovery

Logical subscriptions use a stable fabric-and-node identity. Ephemeral
controller handles never enter durable identity. Recovery is an explicit state
machine:

```text
loss or daemon restart
  -> persist subscription and affected state as stale
  -> perform at most the configured number of targeted gap reads
  -> resubscribe using the same logical subscription ID
  -> on failure, wait with deterministic bounded exponential backoff and jitter
  -> established, or repair_required after the fixed attempt budget
```

Attribute selections are non-empty, duplicate-free, and limited to 256 paths.
The controller port has no wildcard request type, so wildcard expansion must
happen before the bounded boundary. Sleepy devices use a minimum explicit-read
interval and visible staleness instead of aggressive polling.

## Evidence

- `homemagic-application/tests/matter_projection_contract.rs` covers the rule
  matrix, disabled later rules, stable IDs, invalidation, report ordering,
  duplicates, data-version wrap, causal confirmation, resource bounds, restart,
  sleepy-device reads, gap recovery, and exhausted retries.
- `homemagic-matter/tests/controller_contract.rs` proves the committed simulator
  light and lock descriptors project to `on_off.v1` and `access_control.v1`.
- Domain descriptor tests reapply attribute and command bounds during validated
  construction and deserialization.
- The repository secret scan confirms public fixtures and evidence contain no
  raw cluster-write request type.
