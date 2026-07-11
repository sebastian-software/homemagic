---
id: EPIC-003-ISSUES
epic: EPIC-003
title: Agent-Authored Automation Engine issue index
status: in_progress
priority: critical
depends_on: [EPIC-002]
adrs: [ADR-0004, ADR-0017, ADR-0018, ADR-0019, ADR-0020]
created: 2026-07-11
updated: 2026-07-11
---

# EPIC-003 Issue Index

| Issue | Status | Depends on | Outcome |
| --- | --- | --- | --- |
| [E3-001](E3-001-automation-decisions.md) | Ready | EPIC-002 contracts | Accepted automation ADRs |
| [E3-002](E3-002-automation-domain.md) | Planned | E3-001 | Typed IR, lifecycle, plan, and schema |
| [E3-003](E3-003-validation-compiler.md) | Planned | E3-002 | Resolver, validator, Safety Profiles, reducer |
| [E3-004](E3-004-automation-storage.md) | Planned | E3-002 | Durable versions, runs, timers, trace, retention |
| [E3-005](E3-005-deterministic-simulator.md) | Planned | E3-003 | Virtual-time side-effect-free simulation |
| [E3-006](E3-006-runtime-scheduler.md) | Planned | E3-003, E3-004 | Durable interpreter and scheduler |
| [E3-007](E3-007-automation-rpc.md) | Planned | E3-004, E3-005, E3-006 | Governance and authenticated RPC |
| [E3-008](E3-008-automation-exit-audit.md) | Planned | E3-007 | Operations and exit evidence |

## Progress log

- 2026-07-11: User-approved engine design committed as `9eab4c2`.
- 2026-07-11: Dependency-ordered implementation plan and issue set created;
  E3-001 is ready.
