---
id: EPIC-002-ISSUES
epic: EPIC-002
title: Safe Command Control Plane issue index
status: in_progress
priority: critical
depends_on: [EPIC-001]
adrs: [ADR-0013, ADR-0014, ADR-0015, ADR-0016]
created: 2026-07-11
updated: 2026-07-11
---

# EPIC-002 Issue Index

| Issue | Status | Depends on | Outcome |
| --- | --- | --- | --- |
| [E2-001](E2-001-command-decisions.md) | Done | EPIC-001 | Accepted safety ADRs |
| [E2-002](E2-002-command-domain.md) | Done | E2-001 | Typed command state machine |
| [E2-003](E2-003-command-storage.md) | Done | E2-002 | Durable idempotency and audit |
| [E2-004](E2-004-actor-policy.md) | Done | E2-002, E2-003 | Authentication and policy |
| [E2-005](E2-005-command-orchestrator.md) | Done | E2-003, E2-004 | Single command path |
| [E2-006](E2-006-shelly-dispatch.md) | Done | E2-005 | Switch, dimmer, cover dispatch |
| [E2-007](E2-007-command-rpc.md) | Done | E2-005, E2-006 | Authenticated JSON-RPC surface |
| [E2-008](E2-008-command-exit-audit.md) | In progress | E2-007 | Hardware, threat, exit evidence |

## Progress log

- 2026-07-11: Dependency-ordered implementation issues created.
- 2026-07-11: E2-001 decisions accepted; E2-002 is ready.
- 2026-07-11: E2-002 completed; E2-003 is ready.
- 2026-07-11: E2-003 completed with schema v2 and command storage safety
  contracts; E2-004 is ready.
- 2026-07-11: E2-004 policy evaluation and command admission limits completed;
  actor token bootstrap and transport authentication remain.
- 2026-07-11: E2-004 completed with Argon2id actor lifecycle, authenticated RPC
  and WebSocket transport, spoof-proof causation, and policy-matrix canaries;
  E2-005 is ready.
- 2026-07-11: E2-005 completed with the single durable command path, committed
  typed audit facts, post-await deadlines, cancellation, and no-redispatch
  restart recovery; E2-006 is ready.
- 2026-07-11: E2-006 typed private Switch, Light, and Cover RPC mapping plus
  structured safety error normalization completed; transport and confirmation
  remain.
- 2026-07-11: E2-006 completed with typed bounded HTTP dispatch, Digest
  authentication, push-first observed confirmation, one bounded read fallback,
  concrete toggle targets, and duplicate-prevention fixtures; E2-007 is ready.
- 2026-07-11: E2-007 completed with authenticated command/query RPCs, durable
  transition events, daemon recovery wiring, bounded actor-owned history, and a
  safe query-to-command helper; E2-008 is ready.
- 2026-07-11: E2-008 automated safety, threat, recovery, grant, and redacted
  hardware-harness evidence completed; supervised physical reports remain.
