---
id: E2-001
epic: EPIC-002
title: Accept command control-plane decisions
status: done
priority: critical
depends_on: [EPIC-001]
adrs: [ADR-0013, ADR-0014, ADR-0015, ADR-0016]
created: 2026-07-11
updated: 2026-07-11
---

# E2-001: Command Decisions

## Tasks

- [x] Decide RPC authentication and durable actor identity.
- [x] Decide command idempotency, recovery, and retention.
- [x] Decide risk-class policy defaults and bypass rules.
- [x] Decide the EPIC-002 transport strategy.

## Acceptance criteria

- [x] Every physical mutation has one authenticated, persisted, policy-governed path.
- [x] Restart and retry behavior cannot blindly duplicate physical dispatch.
- [x] No adapter or transport can bypass the application command service.
