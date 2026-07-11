---
id: E3-004
epic: EPIC-003
title: Persist automation versions, runs, timers, and trace
status: done
priority: critical
depends_on: [E3-002]
adrs: [ADR-0007, ADR-0020]
created: 2026-07-11
updated: 2026-07-11
---

# E3-004: Automation Storage

## Tasks

- [x] Add forward-only schema migration and historical migration fixture.
- [x] Add application-owned automation repository contracts.
- [x] Persist identity, draft revisions, immutable versions, and plan hashes.
- [x] Persist validation, simulation, rejection, and approval evidence by exact hash.
- [x] Atomically activate or rollback the active-version pointer.
- [x] Persist occurrences, queues, runs, variables, timers, and ordered trace.
- [x] Add optimistic draft and run version checks.
- [x] Add bounded query and restart-recovery methods.
- [x] Implement independent retention with reference protection.
- [x] Test rollback, reopen, conflict, ordering, recovery, and retention invariants.

## Acceptance criteria

- [x] Active content and historical versions are immutable.
- [x] Activation cannot consume evidence for different content or registry revision.
- [x] Pending work survives restart without duplicate run or timer creation.
- [x] Retention never removes active/pending/referenced state.

## Evidence

- Application-owned `AutomationRepository` contracts and typed governance,
  recovery, optimistic conflict, and retention records.
- Forward-only checksum-protected migration `0003_automation_engine.sql` plus a
  committed schema-v2 upgrade fixture.
- SQLite recomputes canonical document/plan hashes and binds validation,
  simulation, approval, activation, and rollback to exact evidence.
- Contract tests cover immutable conflicts, draft/run revisions, exact approval,
  atomic pointer rollback, idempotent restart work, timer/occurrence state
  machines, contiguous trace, reopen recovery, and dependent retention order.
- Detailed contract: [Automation Storage Contract](../../architecture/automation-storage.md).
