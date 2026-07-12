# Matter Node Workflows

## Commissioning admission

`MatterNodeWorkflowService` is the application-owned boundary for Track A node
lifecycle work. It composes authenticated administration, durable Matter state,
and the SDK-neutral controller port. The current slice accepts only the
deterministic simulator and therefore proves application semantics, not Matter
protocol interoperability or physical-device compatibility.

Commissioning starts without setup bytes. `start_commission` reloads the actor,
requires the exact installation-scoped `matter_commission_node` grant, derives
the installation's stable fabric ID, verifies durable active fabric metadata,
and commits an actor-bound `requested` operation. Only after that operation is
returned may a caller construct `MatterCommissioningInput` for execution.

The sensitive input is non-serializable and its `Debug` representation is
redacted. Consuming it creates `MatterCommissioningRequest` directly at the
controller boundary. Setup bytes are not part of the operation target,
idempotency digest, SQLite data, events, logs, or ordinary diagnostics.

## Target semantics

ADR-0040 makes the operation target match facts known at admission time:

- commissioning targets `Fabric` because the authoritative node ID does not yet
  exist;
- cancellation targets `Operation` because it acts on an existing commissioning
  attempt;
- removal targets `Node` only after the controller returned its operational ID.

Operation targets remain immutable. A successful commissioning result will use
the schema 10 operation-to-node relation rather than rewriting historical
request facts.

## Durable result boundary

Schema 10 adds `matter_operation_node_results`. Each future row links exactly
one commissioning operation to a stored fabric-scoped node and stable common
device. Foreign keys require the operation, node, and device to exist. The
repository currently exposes a typed read contract; E4-007-03-02 will add the
single atomic write that commits the node, projections, subscriptions, result,
and terminal operation progress together.

## Commissioning execution

`run_commission` reloads operation ownership and current authority, persists
`validating_setup`, and only then consumes `MatterCommissioningInput`. The
controller's bounded event page must contain the exact declared commissioning
phase sequence for that operation. Missing, duplicate, or reordered phases
become structured repair-required evidence instead of being inferred.

After the controller returns an authoritative descriptor, HomeMagic applies the
same versioned projection rules used everywhere else, performs one bounded read
for the selected scalar paths, and establishes one stable logical subscription.
The read supplies real initial on/off or lock state; no default state is
invented for the common device snapshot.

One repository transaction then writes or updates the stable Matter integration
and enrolled common device, inserts the node descriptor, capability projections,
established subscription, immutable operation-to-node result, and completed
operation progress. A failure at any point rolls back every newly visible node
fact. A second attempt to commission an already-present simulator node ends as a
structured conflict and cannot duplicate common identities.

## Verification

SQLite contracts cover allowed, denied, duplicate, conflicting-key,
inactive-fabric, light and lock projection, actual initial state, subscription,
atomic rollback, reopen, and setup-canary behavior. Unit contracts reject
skipped, reordered, and duplicate controller phases. Historical migration
fixtures cover schema 9 to schema 10. Full workspace tests, all-feature strict
Clippy, Matter dependency boundaries, and the repository secret scan remain
required before each committed child slice closes.
