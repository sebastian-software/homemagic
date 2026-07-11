---
id: E3-004
epic: EPIC-003
title: Persist automation versions, runs, timers, and trace
status: planned
priority: critical
depends_on: [E3-002]
adrs: [ADR-0007, ADR-0020]
created: 2026-07-11
updated: 2026-07-11
---

# E3-004: Automation Storage

## Tasks

- [ ] Add forward-only schema migration and historical migration fixture.
- [ ] Add application-owned automation repository contracts.
- [ ] Persist identity, draft revisions, immutable versions, and plan hashes.
- [ ] Persist validation, simulation, rejection, and approval evidence by exact hash.
- [ ] Atomically activate or rollback the active-version pointer.
- [ ] Persist occurrences, queues, runs, variables, timers, and ordered trace.
- [ ] Add optimistic draft and run version checks.
- [ ] Add bounded query and restart-recovery methods.
- [ ] Implement independent retention with reference protection.
- [ ] Test rollback, reopen, conflict, ordering, recovery, and retention invariants.

## Acceptance criteria

- [ ] Active content and historical versions are immutable.
- [ ] Activation cannot consume evidence for different content or registry revision.
- [ ] Pending work survives restart without duplicate run or timer creation.
- [ ] Retention never removes active/pending/referenced state.
