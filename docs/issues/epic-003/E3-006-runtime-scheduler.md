---
id: E3-006
epic: EPIC-003
title: Execute active automation plans durably
status: planned
priority: critical
depends_on: [E3-003, E3-004]
adrs: [ADR-0018, ADR-0019]
created: 2026-07-11
updated: 2026-07-11
---

# E3-006: Runtime and Scheduler

## Tasks

- [ ] Implement the shared durable step interpreter.
- [ ] Subscribe only active versions to durable normalized events.
- [ ] Persist run intent before interpreting work.
- [ ] Persist each step, variables, timer, command ID, and outcome before continuation.
- [ ] Submit physical actions exclusively through `CommandService`.
- [ ] Implement single, restart, bounded queued, and bounded parallel modes.
- [ ] Implement same-timestamp ordering and self-trigger suppression.
- [ ] Persist missed/skipped occurrences and explicit catch-up runs.
- [ ] Recover timers, queues, runs, and interrupted commands without blind resubmit.
- [ ] Isolate run failures from other automations and device sessions.

## Acceptance criteria

- [ ] Runtime and simulator make equivalent decisions for identical histories.
- [ ] Restart cannot duplicate a dispatched command or schedule occurrence.
- [ ] Missed schedules never execute without an explicit new catch-up request.
- [ ] Queue, parallelism, trace, retry, and duration bounds hold under load.

## Progress

- 2026-07-11: Added deterministic occurrence, run, timer, and trace identity
  derivation plus bounded active-version and direct run/timer recovery queries.
  This prevents restart from inventing duplicate durable work before the step
  coordinator runs. See [Runtime Recovery Keys](../../architecture/automation-runtime-recovery.md).
- 2026-07-11: Added active-only IANA schedule materialization, deterministic
  occurrence recovery, permanent missed/skipped transitions, bounded run-mode
  admission, run-intent-before-interpretation, and expired-timer readiness. An
  SQLite-backed repeated-window test proves restart does not duplicate a run.
- 2026-07-11: Added atomic `AutomationStepWrite` persistence for one optimistic
  run revision, contiguous trace batch, and timer creates/transitions. A forced
  trace failure proves the preceding run/timer changes roll back together.
- 2026-07-11: Extracted the simulator's expression, comparison, boolean, typed
  observation, and IANA-time-window decisions into the shared
  `AutomationEvaluationContext` evaluator. Continuous duration remains an
  explicit host policy so runtime can persist timers instead of blocking.
- 2026-07-11: Added the first one-node-per-commit runtime path for pending
  acceptance, variables, branches, joins, completion, and durable delays.
  SQLite evidence closes and reopens the repository between timer creation and
  consumption, then proves one terminal run with a contiguous four-step trace.
- 2026-07-11: Routed command nodes exclusively through CommandService with
  deterministic run/node/target/attempt idempotency and a durable deadline.
  The end-to-end crash-window test pre-dispatches the exact command without an
  automation checkpoint, then proves runtime recovery records the same command
  ID while the physical dispatcher remains at one call.
