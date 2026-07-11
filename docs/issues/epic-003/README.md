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
| [E3-001](E3-001-automation-decisions.md) | Done | EPIC-002 contracts | Accepted automation ADRs |
| [E3-002](E3-002-automation-domain.md) | Done | E3-001 | Typed IR, lifecycle, plan, and schema |
| [E3-003](E3-003-validation-compiler.md) | Done | E3-002 | Resolver, validator, Safety Profiles, reducer |
| [E3-004](E3-004-automation-storage.md) | Done | E3-002 | Durable versions, runs, timers, trace, retention |
| [E3-005](E3-005-deterministic-simulator.md) | Done | E3-003 | Virtual-time side-effect-free simulation |
| [E3-006](E3-006-runtime-scheduler.md) | Ready | E3-003, E3-004 | Durable interpreter and scheduler |
| [E3-007](E3-007-automation-rpc.md) | Planned | E3-004, E3-005, E3-006 | Governance and authenticated RPC |
| [E3-008](E3-008-automation-exit-audit.md) | Planned | E3-007 | Operations and exit evidence |

## Progress log

- 2026-07-11: User-approved engine design committed as `9eab4c2`.
- 2026-07-11: Dependency-ordered implementation plan and issue set created;
  E3-001 is ready.
- 2026-07-11: E3-001 decisions accepted and indexed; E3-002 is ready.
- 2026-07-11: E3-002 immutable IR, plan, lifecycle, bounds, hashes, schema,
  examples, property tests, and persisted contracts completed; E3-003 and E3-004
  are ready.
- 2026-07-11: E3-003 side-effect-free validation, stable resolution, type
  checking, Safety Profiles, desired-state reduction, deterministic compilation,
  and the published plan contract completed; E3-005 is ready.
- 2026-07-11: E3-004 forward-only automation storage, exact-hash governance,
  atomic activation/rollback, restart recovery, trace ordering, optimistic
  conflicts, and independent reference-protected retention completed; E3-006 is
  ready.
- 2026-07-11: E3-005 data-only virtual-time simulation, typed synthetic
  triggers/state/outcomes, deterministic schedule/DST behavior, missed/catch-up
  semantics, complete node/failure-policy interpretation, and byte-stable trace
  evidence completed.
