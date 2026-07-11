---
id: E1-008
epic: EPIC-001
title: Expose durable reads, event streams, metadata, and repairs
status: done
priority: high
depends_on: [E1-003, E1-007]
adrs: [ADR-0003, ADR-0012]
created: 2026-07-11
updated: 2026-07-11
---

# E1-008: Read API and Repairs

## Outcome

RPC clients can query durable device state and health, subscribe to normalized
events, manage human metadata without changing identity, and diagnose repairs.

## Tasks

- [x] Extend `system.health` with database and migration health.
- [x] Add lifecycle, availability, freshness, integration, and space filters to
      `devices.list`.
- [x] Add connection, freshness, and diagnostic summaries to `devices.get`.
- [x] Add naming, alias, and space-assignment methods.
- [x] Add repair-record list and detail methods.
- [x] Add a bounded server-streaming event subscription prototype.
- [x] Specify cursor, lag, disconnect, and resubscription behavior.
- [x] Update JSON-RPC documentation and examples.

## Acceptance criteria

- [x] Mutating display metadata does not change any stable identity.
- [x] Filters return deterministic, documented results.
- [x] Connection failures are visible as structured data, not log-only text.
- [x] Subscribers receive typed lifecycle and observation events in order.
- [x] Slow subscribers are bounded and receive an explicit lag signal.

## Verification

- [x] RPC dispatch tests for success, invalid parameters, and missing records.
- [x] Metadata identity-stability integration test.
- [x] Event order, cursor resume, lag, and disconnect tests.

## Progress log

- 2026-07-11: Issue created.
- 2026-07-11: Accepted ADR-0012 for durable-cursor JSON-RPC WebSocket
  subscriptions with bounded wake-ups and explicit lag signaling.
- 2026-07-11: Added backend-neutral health and cursor-page repository contracts,
  SQLite cursor reads, and retention-floor validation.
- 2026-07-11: Added filtered device reads, durable metadata mutations, device
  details, repair reads, and structured JSON-RPC errors.
- 2026-07-11: Added `/rpc/ws` subscription negotiation, durable catch-up,
  ordered notifications, explicit lag signaling, and resume/disconnect tests.
- 2026-07-11: Verified with workspace format, Clippy, unit and integration tests,
  and documentation tests.
