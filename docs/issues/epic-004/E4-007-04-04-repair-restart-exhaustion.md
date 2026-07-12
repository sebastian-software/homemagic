---
id: E4-007-04-04
epic: EPIC-004
parent: E4-007-04
title: Reconcile subscription repair restart and exhaustion
status: planned
priority: high
depends_on: [E4-007-04-03]
adrs: [ADR-0014, ADR-0033, ADR-0034, ADR-0041]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-04-04: Repair Restart and Exhaustion

## Outcome

Every durable subscription-repair checkpoint resumes only proven local work or
ends with explicit repair guidance; controller work is never blindly replayed.

## Tasks

- [ ] Persist attempt counters and deadlines before each retryable call.
- [ ] Reconcile gap-read and subscribe checkpoints from bounded evidence.
- [ ] Resume waiting and local commit phases without consuming extra attempts.
- [ ] Preserve exhausted repair records and stable remediation after reopen.
- [ ] Add retention coverage that cannot delete unresolved repair evidence.

## Acceptance criteria

- [ ] Restart cannot reset or exceed recovery budgets.
- [ ] Unknown post-dispatch outcomes become `repair_required`.
- [ ] No automatic catch-up or unrelated command replay occurs.

## Verification

- [ ] Phase-by-phase restart, deadline, exhaustion, retention, and reopen tests
  pass.
- [ ] Full local gates and public Linux x86_64/macOS ARM64 CI pass.
