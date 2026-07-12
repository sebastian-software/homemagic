# ADR-0032: Stream automation transitions as general durable events

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0009, ADR-0012, ADR-0013, ADR-0030, E3-007

## Context

The durable event stream originally required every event to reference a device.
Automation version, operational, and run transitions are installation-domain
facts, not device facts. Assigning a synthetic device would pollute identity,
retention, foreign-key, and trigger semantics.

Automation transitions also need the same restart-safe cursor and bounded
WebSocket delivery contract as device and command events. A second stream would
force agents to coordinate cursors and ordering across APIs.

## Decision

Schema migration 0005 makes the event table's device reference nullable while
preserving all existing cursor values. `DomainEvent.device_id` becomes optional
and adds typed version, operational, and run transition payloads.

The SQLite automation repository appends each transition event in the same
transaction as its authoritative state mutation. Events contain stable IDs,
states, revisions, timestamps, and causation only; authored requests,
rationales, plans, traces, vendor data, tokens, and secrets are excluded.

The existing bounded broadcast channel remains a wake-up mechanism only.
Lifecycle services and engine passes signal it after commits that produced
events; subscribers always drain the database by global cursor. Idle waiting
runs do not produce wake-up noise.

Automation triggers ignore events without device subjects unless a future
version explicitly adds an automation-event trigger. WebSocket delivery filters
automation events to the authenticated owner using server-produced causation.
The server still advances across hidden global cursors, so resume ordering stays
well-defined without revealing another actor's event.

## Consequences

- Device and automation facts share one ordered, restart-safe event channel.
- No fake device or protocol identity is introduced.
- State mutation and transition event persistence cannot diverge.
- Clients may observe cursor gaps caused by events they are not authorized to
  see; those gaps are expected global ordering, not data loss.
- Future installation-scoped event kinds can reuse the optional subject model.
