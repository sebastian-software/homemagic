---
id: E2-005
epic: EPIC-002
title: Implement the single durable command path
status: planned
priority: critical
depends_on: [E2-003, E2-004]
adrs: [ADR-0014, ADR-0015]
created: 2026-07-11
updated: 2026-07-11
---

# E2-005: Command Orchestrator

## Tasks

- [ ] Validate target capability, payload, constraints, freshness, and deadline.
- [ ] Persist received, validation, and policy transitions before dispatch.
- [ ] Implement dry-run, idempotent execute, get, and cancellation services.
- [ ] Serialize per-device dispatch and bound actor/device concurrency.
- [ ] Separate acknowledgement from observation-based confirmation.
- [ ] Recover every non-terminal state without blind redispatch.
- [ ] Publish typed command/audit events after commits.

## Acceptance criteria

- [ ] All callers exercise one application service and repository transaction model.
- [ ] Deadlines and cancellation are visible as durable outcomes.
- [ ] A post-dispatch crash cannot cause automatic duplicate actuation.
