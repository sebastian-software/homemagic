---
id: E4-007-04-02
epic: EPIC-004
parent: E4-007-04
title: Derive durable subscription freshness and repair guidance
status: done
priority: high
depends_on: [E4-007-04-01]
adrs: [ADR-0033, ADR-0034, ADR-0041]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-04-02: Subscription Status

## Outcome

Each logical subscription exposes deterministic freshness, retry budget, gap
state, and stable remediation guidance derived from durable facts at an explicit
evaluation time.

## Tasks

- [x] Define subscription diagnostic status and freshness DTOs.
- [x] Persist bounded recovery counters, retry deadline, and last gap-read time.
- [x] Derive established, stale, waiting, exhausted, and repair-required status.
- [x] Preserve sleepy-device read throttling in status calculations.
- [x] Expose stable remediation codes without adapter text.

## Acceptance criteria

- [x] Status evaluation has no wall-clock or I/O side effects.
- [x] Retry and gap budgets cannot reset through reads or restart.
- [x] Exhaustion remains visible until explicit repair succeeds.

## Verification

- [x] Fresh, stale, sleepy, waiting, exhausted, and reopen matrices pass.
- [x] Boundary timestamps and retry counters are deterministic.
- [x] Full local workspace, strict Clippy, boundary, and secret-scan gates pass.
- [x] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: E4-007-04-01 completed with public cross-platform CI. This issue
  is ready.
- 2026-07-12: Implemented the durable bounded recovery checkpoint, pure status
  projection, versioned diagnostic fields, stable remediation codes, exact
  retry/sleepy boundaries, and restart-preserving contract matrix. Targeted
  contracts and strict Clippy pass.
- 2026-07-12: All 45 Matter repository contracts, historical migrations, the
  all-feature workspace, strict Clippy, Matter boundaries, and secret scan pass.
  Commits `e11e259` and `33a7a20` passed public CI run `29206426040` on Linux
  x86_64 and macOS ARM64. This slice is done.
