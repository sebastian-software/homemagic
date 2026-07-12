---
id: E4-006-03
epic: EPIC-004
parent: E4-006
title: Implement governed Matter dispatch and confirmation adapters
status: done
priority: critical
depends_on: [E4-006-01, E4-006-02]
adrs: [ADR-0014, ADR-0015, ADR-0034, ADR-0036]
created: 2026-07-12
updated: 2026-07-12
---

# E4-006-03: Matter Command Adapters

## Outcome

The common command service dispatches only admitted `on_off.v1` and
`access_control.v1` payloads through the SDK-neutral controller port, while
acknowledgement remains distinct from observation-backed confirmation.

## Tasks

- [x] Implement a Matter `CommandDispatcher` using projection and desired-slot
  identity rather than caller-provided protocol paths.
- [x] Translate only explicit On/Off Set, Lock, and Unlock payloads.
- [x] Map controller acknowledgement into common adapter acknowledgement without
  treating it as physical confirmation.
- [x] Implement `CommandConfirmation` from accepted projected observations and
  one bounded read fallback.
- [x] Normalize mismatch, timeout, subscription loss, and indeterminate restart
  outcomes without redispatch.
- [x] Reconcile toward the latest desired revision after an in-flight command
  reaches an observed terminal outcome.

## Acceptance criteria

- [x] Unsupported common payloads fail closed before controller invocation.
- [x] Acknowledgement alone never produces `confirmed`.
- [x] Restart recovery confirms, fails, times out, or reports indeterminate
  without a second physical invoke.
- [x] Adapter code cannot bypass persistence, policy, desired revision, or audit.

## Verification

- [x] Simulator barriers cover pre-invoke, acknowledgement, and report phases.
- [x] Mismatch, missing report, bounded-read fallback, and restart tests pass.
- [x] Invocation traces contain only typed SDK-neutral controller commands.

## Progress log

- 2026-07-12: E4-006-02 completed durable desired-state coordination and atomic
  dispatch admission; this issue is ready.
- 2026-07-12: Implemented SDK-neutral On/Off and access-control invocation,
  acknowledgement-only adapter results, projected observation confirmation,
  one bounded read, mismatch and indeterminate recovery, and no-redispatch
  restart contracts.
