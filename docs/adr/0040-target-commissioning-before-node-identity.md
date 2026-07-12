# ADR-0040: Target commissioning before node identity exists

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0013, ADR-0014, ADR-0033, ADR-0037, ADR-0038, EPIC-004,
  E4-007-03

## Context

A Matter operational node ID is authoritative only after the controller has
commissioned or rediscovered the node. Requiring a `MatterNodeId` when admitting
commissioning would make HomeMagic invent a protocol identity from setup input,
fixture knowledge, or caller input. That would weaken idempotency and allow the
durable operation target to disagree with the commissioned result.

Cancellation has a similar mismatch. It acts on one existing commissioning
operation, not on an assumed node. The current generic `Node` target cannot
express that relationship before commissioning succeeds.

## Decision

`MatterOperationTarget` distinguishes three target families:

- `Fabric` for fabric lifecycle and commissioning requests whose node identity
  is not known yet;
- `Operation` for cancellation of one actor-owned commissioning operation,
  carrying the owning fabric and original operation ID;
- `Node` only after the controller has returned an authoritative fabric-scoped
  node ID.

`CommissionNode` accepts only `Fabric`, `CancelCommissioning` accepts only
`Operation`, and `RemoveNode` accepts only `Node`. Setup payload bytes remain
outside the target, canonical request hash, database, events, logs, and ordinary
diagnostics. The actor-scoped idempotency key distinguishes repeated admission.

After controller success, the application atomically records an immutable
operation-result link to the authoritative node together with node metadata,
capability projections, subscriptions, and terminal progress. The original
operation target remains immutable.

A cancellation operation reauthorizes both itself and the referenced original
operation. Local `requested` commissioning may be cancelled without crossing
the controller boundary. Once controller work has started, cancellation is best
effort and its outcome must update both operations atomically or create explicit
repair evidence.

## Consequences

- No caller or setup parser can choose an authoritative node ID prematurely.
- Commissioning idempotency remains actor- and fabric-scoped before a node
  exists.
- Operation history keeps the original request facts while exposing the
  eventual node through an explicit result relation.
- Cancellation ownership and causation are queryable without overloading a
  fabricated node target.
- Repository contracts need an atomic commissioned-node result write and an
  atomic cancellation reconciliation write.

## Rejected alternatives

- Preallocating a `MatterNodeId` assumes controller behavior not guaranteed by
  the SDK-neutral port.
- Parsing a node ID from setup input treats untrusted onboarding material as
  operational identity.
- Mutating the operation target after success would invalidate the canonical
  request binding and historical audit facts.
- Treating cancellation as a node action cannot identify an in-flight attempt
  that has not produced a node.
