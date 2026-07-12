---
id: E4-007-04-03
epic: EPIC-004
parent: E4-007-04
title: Execute explicit bounded gap-read and resubscribe repair
status: in_progress
priority: high
depends_on: [E4-007-04-02]
adrs: [ADR-0014, ADR-0033, ADR-0034, ADR-0041]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-04-03: Explicit Subscription Repair

## Outcome

An authenticated actor can explicitly admit and run one durable subscription
repair that performs at most the declared gap-read and resubscribe work.

## Tasks

- [x] Admit `RepairSubscription` only for an owned durable node subscription.
- [x] Persist repair intent before marking projections stale or controller I/O.
- [x] Execute bounded gap read through normal report normalization.
- [x] Re-establish the stable logical subscription with a new ephemeral session.
- [x] Atomically persist projections, subscription status, progress, and repair.

## Acceptance criteria

- [x] Diagnostics alone never start this workflow.
- [x] Repair never invokes raw cluster writes or unrelated commands.
- [x] A successful gap read preserves causation and data-version ordering.
- [x] Budget exhaustion ends in explicit `repair_required`.

## Verification

- [x] Success, stale report, gap failure, subscribe retry, exhaustion, duplicate,
  foreign, and atomic rollback tests pass.
- [x] Controller calls never exceed the declared policy.
- [x] Full local workspace, strict Clippy, boundary, and secret-scan gates pass.
- [ ] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: E4-007-04-02 completed with public cross-platform CI. This issue
  is ready.
- 2026-07-12: Implemented actor-bound idempotent admission, atomic stale and
  terminal barriers, normalized bounded gap reads, durable subscribe attempt
  reservation, deterministic waiting, success, and exhaustion. All 50 Matter
  repository contracts and the targeted strict-Clippy gate pass.
- 2026-07-12: The all-feature workspace, strict Clippy, Matter boundaries, and
  secret scan pass. Commit, push, and public CI remain pending.
