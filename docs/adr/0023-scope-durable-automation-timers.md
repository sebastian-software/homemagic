# ADR-0023: Scope durable automation timers by semantic role

- Status: Accepted
- Date: 2026-07-12

## Context

A plan node may own more than one durable time contract. A wait containing a
continuous state-duration condition has both a wait timeout and a stability
timer. Command retry and explicit delay timers also have different recovery
semantics.

Identifying or loading a timer only by run and node is therefore ambiguous.
Deriving identity from run, node, and ready instant can also collide when two
roles become ready at the same millisecond.

## Decision

Every automation timer stores one semantic kind:

- delay;
- wait timeout;
- command retry;
- state duration.

Deterministic timer identity includes the kind's stable scope key in addition
to run ID, node ID, and ready instant. Runtime lookups match both node and kind.
Storage treats kind as immutable during timer transitions.

The older generic identity helper remains for non-runtime compatibility tests,
but production runtime timers always use scoped identity derivation.

## Consequences

- Multiple timers can safely coexist on one plan node.
- Recovery dispatches each ready timer to the correct interpreter behavior.
- Equal ready instants cannot collide across semantic roles.
- Persisted timer payloads remain self-describing for audit and diagnostics.
