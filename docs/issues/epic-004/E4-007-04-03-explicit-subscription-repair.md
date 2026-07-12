---
id: E4-007-04-03
epic: EPIC-004
parent: E4-007-04
title: Execute explicit bounded gap-read and resubscribe repair
status: ready
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

- [ ] Admit `RepairSubscription` only for an owned durable node subscription.
- [ ] Persist repair intent before marking projections stale or controller I/O.
- [ ] Execute bounded gap read through normal report normalization.
- [ ] Re-establish the stable logical subscription with a new ephemeral session.
- [ ] Atomically persist projections, subscription status, progress, and repair.

## Acceptance criteria

- [ ] Diagnostics alone never start this workflow.
- [ ] Repair never invokes raw cluster writes or unrelated commands.
- [ ] A successful gap read preserves causation and data-version ordering.
- [ ] Budget exhaustion ends in explicit `repair_required`.

## Verification

- [ ] Success, stale report, gap failure, subscribe retry, exhaustion, duplicate,
  foreign, and atomic rollback tests pass.
- [ ] Controller calls never exceed the declared policy.

## Progress log

- 2026-07-12: E4-007-04-02 completed with public cross-platform CI. This issue
  is ready.
