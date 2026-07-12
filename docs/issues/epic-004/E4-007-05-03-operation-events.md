---
id: E4-007-05-03
epic: EPIC-004
parent: E4-007-05
title: Stream actor-filtered durable Matter operation events
status: in_progress
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

- [x] Add versioned `MatterOperationTransitioned` domain event data.
- [x] Append creation and phase events in the same SQLite transactions.
- [x] Derive causation actor from the immutable operation binding.
- [x] Filter both replay and live delivery by exact authenticated actor.
- [x] Retain cursor expiry, lag, and reconnect semantics.

## Acceptance criteria

- [x] No transition is visible before its operation state commits.
- [x] Events disclose no target, setup, secret reference, or controller detail.
- [x] Another actor receives neither historical nor live transition events.

## Verification

- [x] Creation, every phase, rollback, actor isolation, cursor, lag, and reopen
  contracts pass.
- [ ] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: Commit `93400b8` added the versioned secret-free event, atomic
  SQLite creation/transition projection, exact actor filtering, and durable
  cursor polling. Complete commissioning phase order, duplicate suppression,
  cancellation rollback, actor isolation, cursor, lag, and reopen contracts
  pass. Full local workspace tests, strict Clippy, Matter boundaries, and secret
  scans pass; public CI remains pending.
