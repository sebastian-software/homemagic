# ADR-0026: Checkpoint active automation events after occurrence materialization

- Status: Accepted
- Date: 2026-07-12

## Context

Event-driven automations must survive restart without losing events or creating
duplicate runs. The normalized foundation stream already provides a monotonic
durable cursor, while automation occurrences and runs have deterministic IDs.
Advancing a consumer cursor before creating all occurrences would lose work;
requiring one cross-module database transaction would couple application ports
and prevent independent adapters.

Schedule and event occurrences can share an identical timestamp. Ordering by a
random-looking UUID would make runtime decisions depend on implementation
details. Retention may also remove events before a stopped consumer catches up,
which must not silently activate newer state.

## Decision

The automation engine owns one optimistic durable event-consumer cursor. For
each event, it performs these steps in cursor order:

1. load the bounded set of exact active automation versions;
2. match their normalized observation, device-event, or command-outcome
   triggers;
3. insert one deterministic occurrence per matching automation and event;
4. record exact self-caused matches directly as suppressed occurrences;
5. advance the consumer cursor only after every occurrence is durable.

Replay between steps 3 and 5 is at-least-once but idempotent because occurrence
identity derives from automation ID, version, and event cursor. A retention gap
is a typed failure and never advances the cursor automatically.

Runtime recovery orders occurrences by `(occurred_at, source_rank,
event_cursor, occurrence_id)`, where schedule is source rank 0 and event is
source rank 1. Event occurrences use an unbounded acceptance end because only
schedule occurrences have missed-time semantics; the durable event itself is
the acceptance fact.

Only exact active versions are subscribed. Self-trigger policy uses the
explicit causation from ADR-0025: same-version compares automation and version,
same-correlation suppresses a causal chain originating from the same automation
across its versions, and allow does not suppress.

## Consequences

- A crash can repeat matching work but cannot lose it or create another run.
- Same-timestamp decisions are stable across restart and platforms.
- Inactive drafts and historical versions cannot receive new occurrences.
- Operators must explicitly resolve an expired cursor instead of receiving a
  surprising automatic catch-up.
- Event processing and plan admission remain separate durable stages.
