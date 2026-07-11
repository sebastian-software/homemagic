---
id: E2-007
epic: EPIC-002
title: Expose authenticated command and audit RPCs
status: planned
priority: high
depends_on: [E2-005, E2-006]
adrs: [ADR-0013, ADR-0016]
created: 2026-07-11
updated: 2026-07-11
---

# E2-007: Command RPC

## Tasks

- [ ] Add authenticated `commands.validate`, `execute`, `get`, and `cancel`.
- [ ] Add bounded command/audit filters and cursor reads.
- [ ] Derive actor exclusively from authentication context.
- [ ] Stream typed command transitions on the durable event channel.
- [ ] Add device-query-to-command CLI examples without hand-built IDs.
- [ ] Document dry-run, idempotency, deadlines, rollback, and emergency stop.
- [ ] Test authentication, validation, policy, idempotency, and error mappings.

## Acceptance criteria

- [ ] RPC and internal calls produce identical decisions/outcomes.
- [ ] Tokens and sensitive fields never appear in responses, events, or logs.
- [ ] API examples are executable from a clean checkout.
