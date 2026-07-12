---
id: E4-007-04-02
epic: EPIC-004
parent: E4-007-04
title: Orchestrate explicit bounded subscription repair
status: planned
priority: high
depends_on: [E4-007-04-01]
adrs: [ADR-0014, ADR-0033, ADR-0034]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-04-02: Explicit Subscription Repair

## Outcome

An authenticated operator or agent can explicitly admit one bounded gap-read
and resubscribe attempt. The operation either atomically restores projection and
subscription health or leaves stable repair-required evidence without acquiring
automatic catch-up permission.

## Tasks

- [ ] Admit actor-bound idempotent `RepairSubscription` operations for durable
  nodes with logical subscriptions.
- [ ] Persist `reading_gap` before one bounded selected read.
- [ ] Normalize refreshed reports through the accepted projection rules.
- [ ] Persist `subscribing` before one bounded resubscribe attempt.
- [ ] Atomically commit refreshed projections, established subscription, and
  completed operation progress.
- [ ] Atomically mark subscription and operation repair-required on exhausted or
  indeterminate outcomes.
- [ ] Reconcile restart checkpoints without blind reads or resubscription.

## Acceptance criteria

- [ ] Repair begins only from an explicit authenticated request.
- [ ] No wildcard read, raw cluster write, or unbounded retry is possible.
- [ ] Failure remains visible with stable structured remediation guidance.
- [ ] Completed replay performs no controller work.

## Verification

- [ ] Happy path, read failure, subscribe failure, retry exhaustion,
  idempotency, foreign, atomic rollback, and reopen tests pass.
- [ ] Restart at `reading_gap` and `subscribing` reaches an explicit terminal
  outcome.
