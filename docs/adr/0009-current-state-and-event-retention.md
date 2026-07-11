# ADR-0009: Separate current state from bounded event retention

- Status: Accepted
- Date: 2026-07-11

## Context

HomeMagic needs current device state immediately after restart and needs recent
events for diagnostics, subscriptions, and later automation processing. An
unbounded event log would turn an embedded controller into an analytical
time-series database. Expiring current state together with history would make a
temporarily quiet or unreachable device appear unknown after restart.

## Decision

Current-state projections and immutable event history have independent storage
and retention policies.

### Current state

The latest device, endpoint, capability, availability, diagnostic, and
observation snapshots are retained for the lifetime of an enrolled device.
Freshness is represented by timestamps and calculated status; expiration never
changes an old observed value into a guessed value.

Explicitly removed devices retain an identity tombstone and minimal provenance
for 90 days so rediscovery and delayed events cannot silently create duplicate
identity. A future administrative operation may retain a tombstone longer, but
automatic pruning never removes active or stale enrolled-device snapshots.

### Events and operational records

EPIC-001 uses these defaults per installation:

- normalized observation and lifecycle events: 30 days and 250,000 rows;
- refresh and connection diagnostics: 14 days and 50,000 rows;
- resolved repair records: 30 days after resolution;
- unresolved repair records: retained until resolved or explicitly dismissed.

For categories with both an age and row bound, the earlier limit wins. Limits
are configurable within validated safety bounds. Command, policy, automation,
and security audit retention are intentionally deferred to the epics that define
those records and are not shortened by this ADR.

Events receive a monotonic installation-local cursor when committed. Retention
records the lowest available cursor. A subscription or query requesting an older
cursor receives a typed `cursor_expired` result containing the earliest
available cursor; it never silently resumes at an arbitrary position.

Pruning runs after successful writes and on a periodic maintenance schedule in
bounded transactions of at most 1,000 rows. Pruning failure does not roll back
the state update that triggered maintenance, but it is exposed through database
health and diagnostics.

Backups contain current projections, identity tombstones, and the retained event
window. Restore preserves cursor values and the retention floor.

## Consequences

- Current state remains available after event history is pruned.
- Database growth is bounded for normal device telemetry.
- Consumers must handle expired cursors explicitly.
- HomeMagic is not a long-term analytical telemetry store; exports can be added
  later without changing the current-state contract.
