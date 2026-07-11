---
id: E2-005
epic: EPIC-002
title: Implement the single durable command path
status: done
priority: critical
depends_on: [E2-003, E2-004]
adrs: [ADR-0014, ADR-0015]
created: 2026-07-11
updated: 2026-07-11
---

# E2-005: Command Orchestrator

## Tasks

- [x] Validate target capability, payload, constraints, freshness, and deadline.
- [x] Persist received, validation, and policy transitions before dispatch.
- [x] Implement dry-run, idempotent execute, get, and cancellation services.
- [x] Serialize per-device dispatch and bound actor/device concurrency.
- [x] Separate acknowledgement from observation-based confirmation.
- [x] Recover every non-terminal state without blind redispatch.
- [x] Publish typed command/audit events after commits.

## Acceptance criteria

- [x] All callers exercise one application service and repository transaction model.
- [x] Deadlines and cancellation are visible as durable outcomes.
- [x] A post-dispatch crash cannot cause automatic duplicate actuation.

## Progress log

- 2026-07-11: Added the transport-neutral `CommandService` and explicit dispatch,
  confirmation, post-commit audit, repository, clock, policy, and capacity boundaries.
- 2026-07-11: Added typed target/payload/deadline/precondition validation,
  retry-stable canonical hashing, dry-run, cancellation, actor ownership, and
  actual target-risk evaluation before durable dispatch.
- 2026-07-11: Added post-await deadline checks and bounded restart recovery.
  `received` and `validated` work may resume after current policy evaluation;
  `dispatched` and `acknowledged` work only performs observation confirmation and
  is never blindly dispatched again.
- 2026-07-11: End-to-end SQLite tests prove ordered receipt, validation, policy,
  dispatch, acknowledgement, confirmation, timeout, cancellation, idempotent
  retry, post-commit audit fan-out, and all non-terminal recovery states.
