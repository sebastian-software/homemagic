# ADR-0041: Separate Matter diagnostics from explicit repair

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0013, ADR-0014, ADR-0029, ADR-0033, ADR-0034, EPIC-004,
  E4-007-04

## Context

Matter diagnostics need enough durable and controller evidence to explain stale
subscriptions, report gaps, retries, and repair guidance. If the same read path
also mutates state, an operator or agent inspecting a problem could
unexpectedly resubscribe, perform gap reads, or consume a retry budget.

HomeMagic also deliberately does not catch up missed automation work unless a
caller requests it. Subscription recovery needs the same predictable boundary:
observation may recommend repair, but must not silently authorize it.

## Decision

Diagnostics are authenticated, bounded, redacted, and read-only. They may
combine durable fabric, node, endpoint, projection, operation, repair, and
subscription facts with one bounded controller status or inventory query, but
they cannot invoke, write attributes, resubscribe, run a gap read, or transition
an operation.

Repair is a separate actor-bound `RepairSubscription` administration operation
targeted at one durable node. Admission revalidates the exact installation grant
and persists intent before controller I/O. Execution consumes a fixed retry and
gap-read budget, persists every phase and attempt, and exposes `completed` or
`repair_required` explicitly. It never grants automation schedule catch-up or
replays unrelated commands.

Sensitive setup data, fabric secret references, native network material, raw
SDK objects, and unrestricted cluster mutation remain absent from both DTOs and
ordinary operation facts.

## Consequences

- Agents can inspect state repeatedly without surprising writes.
- Repair requires an explicit separately auditable request.
- Retry exhaustion and restart state remain durable and queryable.
- Diagnostics and repair can share normalized DTOs without sharing mutation
  entry points.

## Rejected alternatives

- Repair-on-read makes harmless inspection state-changing and consumes bounded
  resources unexpectedly.
- Automatic background catch-up hides causation and conflicts with ADR-0029.
- Exposing raw controller or cluster APIs bypasses projection and policy
  boundaries.
