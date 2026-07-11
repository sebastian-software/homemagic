---
id: E1-008
epic: EPIC-001
title: Expose durable reads, event streams, metadata, and repairs
status: planned
priority: high
depends_on: [E1-003, E1-007]
adrs: [ADR-0003]
created: 2026-07-11
updated: 2026-07-11
---

# E1-008: Read API and Repairs

## Outcome

RPC clients can query durable device state and health, subscribe to normalized
events, manage human metadata without changing identity, and diagnose repairs.

## Tasks

- [ ] Extend `system.health` with database and migration health.
- [ ] Add lifecycle, availability, freshness, integration, and space filters to
      `devices.list`.
- [ ] Add connection, freshness, and diagnostic summaries to `devices.get`.
- [ ] Add naming, alias, and space-assignment methods.
- [ ] Add repair-record list and detail methods.
- [ ] Add a bounded server-streaming event subscription prototype.
- [ ] Specify cursor, lag, disconnect, and resubscription behavior.
- [ ] Update JSON-RPC documentation and examples.

## Acceptance criteria

- [ ] Mutating display metadata does not change any stable identity.
- [ ] Filters return deterministic, documented results.
- [ ] Connection failures are visible as structured data, not log-only text.
- [ ] Subscribers receive typed lifecycle and observation events in order.
- [ ] Slow subscribers are bounded and receive an explicit lag signal.

## Verification

- [ ] RPC dispatch tests for success, invalid parameters, and missing records.
- [ ] Metadata identity-stability integration test.
- [ ] Event order, cursor resume, lag, and disconnect tests.

## Progress log

- 2026-07-11: Issue created.
