---
id: E2-002
epic: EPIC-002
title: Define typed command and policy contracts
status: ready
priority: critical
depends_on: [E2-001]
adrs: [ADR-0014, ADR-0015]
created: 2026-07-11
updated: 2026-07-11
---

# E2-002: Command Domain

## Tasks

- [ ] Define stable actor, command, idempotency, and policy identifiers.
- [ ] Define versioned typed on/off, level, and position command payloads.
- [ ] Define the complete command state machine and terminal-state invariants.
- [ ] Separate requested, acknowledged, and observed-confirmed state.
- [ ] Define deadlines, cancellation, expected-state preconditions, and errors.
- [ ] Define policy grants, inputs, decisions, and stable reason codes.
- [ ] Add serialization and property/state-transition tests.

## Acceptance criteria

- [ ] Invalid transitions and payload/target schema mismatches are unrepresentable or rejected.
- [ ] Vendor-specific RPC dictionaries do not appear in public command contracts.
- [ ] Persisted contracts round-trip without runtime-only context.
