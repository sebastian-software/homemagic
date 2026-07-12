---
id: E4-007-04-04
epic: EPIC-004
parent: E4-007-04
title: Reconcile subscription repair restart and exhaustion
status: done
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

- [x] Persist attempt counters and deadlines before each retryable call.
- [x] Reconcile gap-read and subscribe checkpoints from bounded evidence.
- [x] Resume waiting and local commit phases without consuming extra attempts.
- [x] Preserve exhausted repair records and stable remediation after reopen.
- [x] Add retention coverage that cannot delete unresolved repair evidence.

## Acceptance criteria

- [x] Restart cannot reset or exceed recovery budgets.
- [x] Unknown post-dispatch outcomes become `repair_required`.
- [x] No automatic catch-up or unrelated command replay occurs.

## Verification

- [x] Phase-by-phase restart, deadline, exhaustion, retention, and reopen tests
  pass.
- [x] Full local workspace, strict Clippy, boundary, and secret-scan gates pass.
- [x] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: E4-007-04-03 completed with public cross-platform CI. This issue
  is ready.
- 2026-07-12: Implemented pre-dispatch gap and subscribe reservations,
  fail-closed reconciliation for ambiguous `reading_gap` and `subscribing`,
  requested/waiting/terminal reopen behavior, and unresolved-repair retention.
  Seven targeted repair contracts and strict Clippy pass.
- 2026-07-12: All 52 Matter repository contracts, historical migrations, the
  all-feature workspace, strict Clippy, Matter boundaries, and secret scan pass.
  Commit and push remained pending.
- 2026-07-12: Commits `5434412` and `ac89d39` passed public CI run
  `29207369049` across Linux x86_64 and macOS ARM64. This issue is done.
