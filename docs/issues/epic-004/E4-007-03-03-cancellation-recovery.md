---
id: E4-007-03-03
epic: EPIC-004
parent: E4-007-03
title: Reconcile commissioning cancellation and every restart checkpoint
status: planned
priority: high
depends_on: [E4-007-03-01, E4-007-03-02]
adrs: [ADR-0014, ADR-0033, ADR-0037, ADR-0040]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-03-03: Cancellation and Restart Recovery

## Outcome

Cancellation is immediate before dispatch and best effort afterward. Every
simulated commissioning checkpoint reconciles to completed, cancelled, failed,
or explicit repair-required state without blindly repeating protocol work or
reusing lost setup input.

## Tasks

- [ ] Admit cancellation against one owned commissioning operation.
- [ ] Cancel `requested` commissioning locally without a controller call.
- [ ] Persist a cancellation operation before in-flight controller cancellation.
- [ ] Reconcile cancelled, already-completed, and unknown controller outcomes.
- [ ] Atomically update original and cancellation operations plus repair facts.
- [ ] Recover every nonterminal commissioning phase from bounded controller
  inventory and progress evidence.
- [ ] Fail to `repair_required` when evidence cannot prove a safe outcome.

## Acceptance criteria

- [ ] Cancellation never claims to undo a commissioned node.
- [ ] Foreign operations remain indistinguishable from missing operations.
- [ ] Restart never reuses setup input or blindly calls commission again.
- [ ] Original and cancellation histories cannot contradict each other.

## Verification

- [ ] Pre-dispatch and every in-flight cancellation outcome pass after reopen.
- [ ] Every simulator commissioning restart phase reaches an explicit terminal
  outcome.
- [ ] Atomic conflict and indeterminate-outcome repair tests pass.
