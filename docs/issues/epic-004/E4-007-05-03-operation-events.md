---
id: E4-007-05-03
epic: EPIC-004
parent: E4-007-05
title: Stream actor-filtered durable Matter operation events
status: planned
priority: high
depends_on: [E4-007-05-02]
adrs: [ADR-0012, ADR-0013, ADR-0032, ADR-0042]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-05-03: Operation Events

## Outcome

Every durable Matter operation transition appends one secret-free general event
atomically, and WebSocket cursor replay/live delivery exposes it only to the
actor from the immutable operation binding.

## Tasks

- [ ] Add versioned `MatterOperationTransitioned` domain event data.
- [ ] Append creation and phase events in the same SQLite transactions.
- [ ] Derive causation actor from the immutable operation binding.
- [ ] Filter both replay and live delivery by exact authenticated actor.
- [ ] Retain cursor expiry, lag, and reconnect semantics.

## Acceptance criteria

- [ ] No transition is visible before its operation state commits.
- [ ] Events disclose no target, setup, secret reference, or controller detail.
- [ ] Another actor receives neither historical nor live transition events.

## Verification

- [ ] Creation, every phase, rollback, actor isolation, cursor, lag, and reopen
  contracts pass.
