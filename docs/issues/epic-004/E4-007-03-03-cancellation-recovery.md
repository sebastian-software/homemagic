---
id: E4-007-03-03
epic: EPIC-004
parent: E4-007-03
title: Reconcile commissioning cancellation and every restart checkpoint
status: in_progress
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

- [x] Admit cancellation against one owned commissioning operation.
- [x] Cancel `requested` commissioning locally without a controller call.
- [x] Persist a cancellation operation before in-flight controller cancellation.
- [x] Reconcile cancelled, already-completed, and unknown controller outcomes.
- [x] Atomically update original and cancellation operations plus repair facts.
- [x] Recover every nonterminal commissioning phase from bounded controller
  inventory and progress evidence.
- [x] Fail to `repair_required` when evidence cannot prove a safe outcome.

## Acceptance criteria

- [x] Cancellation never claims to undo a commissioned node.
- [x] Foreign operations remain indistinguishable from missing operations.
- [x] Restart never reuses setup input or blindly calls commission again.
- [x] Original and cancellation histories cannot contradict each other.

## Verification

- [x] Pre-dispatch and every in-flight cancellation outcome pass after reopen.
- [x] Every simulator commissioning restart phase reaches an explicit terminal
  outcome.
- [x] Atomic conflict and indeterminate-outcome repair tests pass.
- [x] Full local workspace, strict Clippy, boundary, and secret-scan gates pass.
- [ ] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: E4-007-03-02 completed with public cross-platform CI. This child
  issue is ready.
- 2026-07-12: Implemented owner-isolated cancellation admission, local
  pre-dispatch cancellation, durable in-flight cancellation, atomic dual-history
  reconciliation, and fail-closed bounded restart recovery. All 37 Matter
  repository contracts, the full all-feature workspace suite, strict Clippy,
  Matter boundary checks, and secret scans pass locally. Commit, push, and
  public CI remain pending.
