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
| [E2-003](E2-003-command-storage.md) | Ready | E2-002 | Durable idempotency and audit |
| [E2-004](E2-004-actor-policy.md) | Planned | E2-002, E2-003 | Authentication and policy |
| [E2-005](E2-005-command-orchestrator.md) | Planned | E2-003, E2-004 | Single command path |
| [E2-006](E2-006-shelly-dispatch.md) | Planned | E2-005 | Switch, dimmer, cover dispatch |
| [E2-007](E2-007-command-rpc.md) | Planned | E2-005, E2-006 | Authenticated JSON-RPC surface |
| [E2-008](E2-008-command-exit-audit.md) | Planned | E2-007 | Hardware, threat, exit evidence |

## Progress log

- 2026-07-11: Dependency-ordered implementation issues created.
- 2026-07-11: E2-001 decisions accepted; E2-002 is ready.
- 2026-07-11: E2-002 completed; E2-003 is ready.
