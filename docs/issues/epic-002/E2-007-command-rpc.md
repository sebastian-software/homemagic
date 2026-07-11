---
id: E2-007
epic: EPIC-002
title: Expose authenticated command and audit RPCs
status: done
priority: high
depends_on: [E2-005, E2-006]
adrs: [ADR-0013, ADR-0016]
created: 2026-07-11
updated: 2026-07-11
---

# E2-007: Command RPC

## Tasks

- [x] Add authenticated `commands.validate`, `execute`, `get`, and `cancel`.
- [x] Add bounded command/audit filters and cursor reads.
- [x] Derive actor exclusively from authentication context.
- [x] Stream typed command transitions on the durable event channel.
- [x] Add device-query-to-command CLI examples without hand-built IDs.
- [x] Document dry-run, idempotency, deadlines, rollback, and emergency stop.
- [x] Test authentication, validation, policy, idempotency, and error mappings.

## Acceptance criteria

- [x] RPC and internal calls produce identical decisions/outcomes.
- [x] Tokens and sensitive fields never appear in responses, events, or logs.
- [x] API examples are executable from a clean checkout.

## Progress log

- 2026-07-11: Added authenticated validate, execute, get, cancel, list, and audit
  methods over the single `CommandService`; requests contain no actor field.
- 2026-07-11: Added bounded actor-owned command filters and command-local audit
  sequence reads. Cross-actor lookup is indistinguishable from absence.
- 2026-07-11: Projected committed command transitions into the durable event
  channel before WebSocket fan-out and wired startup recovery into the daemon.
- 2026-07-11: Added SQLite-backed RPC parity/error tests and the dependency-free
  `scripts/rpc-command.py` query-to-command helper, which validates by default.
- 2026-07-11: Documented idempotency, UTC deadlines, dry-run semantics,
  compensating commands, cover stop, and the required physical emergency path.
