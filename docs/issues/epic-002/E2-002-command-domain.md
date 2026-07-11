---
id: E2-002
epic: EPIC-002
title: Define typed command and policy contracts
status: done
priority: critical
depends_on: [E2-001]
adrs: [ADR-0014, ADR-0015]
created: 2026-07-11
updated: 2026-07-11
---

# E2-002: Command Domain

## Tasks

- [x] Define stable actor, command, idempotency, and policy identifiers.
- [x] Define versioned typed on/off, level, and position command payloads.
- [x] Define the complete command state machine and terminal-state invariants.
- [x] Separate requested, acknowledged, and observed-confirmed state.
- [x] Define deadlines, cancellation, expected-state preconditions, and errors.
- [x] Define policy grants, inputs, decisions, and stable reason codes.
- [x] Add serialization and exhaustive state-transition tests.

## Acceptance criteria

- [x] Invalid transitions and payload/target schema mismatches are unrepresentable or rejected.
- [x] Vendor-specific RPC dictionaries do not appear in public command contracts.
- [x] Persisted contracts round-trip without runtime-only context.

## Progress log

- 2026-07-11: Added common-capability command payloads, durable actor/command/grant
  identities, deadlines, idempotency keys, preconditions, and policy contracts.
- 2026-07-11: Added complete deterministic policy inputs and immutable command
  transition audit records for the persistence boundary.
- 2026-07-11: Exhaustive state-edge, validation, dry-run, serialization, Clippy,
  and documentation tests pass.
