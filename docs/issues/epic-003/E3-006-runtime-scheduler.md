---
id: E3-006
epic: EPIC-003
title: Execute active automation plans durably
status: in_progress
priority: critical
depends_on: [E3-003, E3-004]
adrs: [ADR-0018, ADR-0019, ADR-0021, ADR-0022, ADR-0023, ADR-0024, ADR-0025, ADR-0026, ADR-0027]
created: 2026-07-11
updated: 2026-07-12
---

# E3-006: Runtime and Scheduler

## Tasks

- [ ] Implement the shared durable step interpreter.
- [ ] Subscribe only active versions to durable normalized events.
- [x] Persist run intent before interpreting work.
- [x] Persist each step, variables, timer, command ID, and outcome before continuation.
- [x] Submit physical actions exclusively through `CommandService`.
- [x] Implement single, restart, bounded queued, and bounded parallel modes.
- [ ] Implement same-timestamp ordering and self-trigger suppression.
- [ ] Persist missed/skipped occurrences and explicit catch-up runs.
- [ ] Recover timers, queues, runs, and interrupted commands without blind resubmit.
- [ ] Isolate run failures from other automations and device sessions.

## Acceptance criteria

- [ ] Runtime and simulator make equivalent decisions for identical histories.
- [x] Restart cannot duplicate a dispatched command or schedule occurrence.
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
- 2026-07-11: Added non-blocking durable wait nodes. False conditions create an
  atomic timeout timer, subsequent event-driven steps can cancel it on success,
  and ready timers are consumed with the compiled failure policy. SQLite
  evidence proves a false wait times out, continues, and completes exactly once.
- 2026-07-11: Accepted ADR-0021 and added bounded persistent continuations for
  nested parallel/race groups and group-local stop-branch handling. SQLite
  evidence proves both branches of a parallel group checkpoint independently,
  resume through durable timers, remove the frame, and complete once.
- 2026-07-12: Accepted ADR-0022 and replaced trace/last-ID retry inference with
  an explicit persisted command-attempt state. End-to-end evidence forces a
  transport failure, checkpoints backoff, recreates the runtime, consumes the
  timer, dispatches attempt one exactly once, and completes. Pure tests prove
  partial multi-target retry selects only failed targets and stops at the
  compiled attempt bound.
- 2026-07-12: Accepted ADR-0023 and scoped every runtime timer by semantic kind.
  Delay, wait-timeout, command-retry, and state-duration IDs can no longer
  collide at the same node/instant; storage keeps the role immutable and
  runtime recovery uses kind-safe lookups.
- 2026-07-12: Implemented ADR-0024 continuous-condition intervals in the shared
  runtime evaluator. SQLite evidence persists pending, consumes a ready timer
  into mature, then advances and clears the interval; a pure reset test proves
  a false nested value invalidates the matching canonical condition hash.
- 2026-07-12: Accepted ADR-0025 and propagated exact automation, version, and
  run causation through persisted commands into typed command-transition
  events. Contract evidence also proves resolved endpoint and capability
  identity survive the audit projection for precise outcome-trigger matching.
- 2026-07-12: Accepted ADR-0026 and added migration 0004 plus a cursor-last,
  active-version-only event processor. SQLite evidence proves same-timestamp
  cursor order, replay idempotency, inactive-version exclusion, exact
  self-cause suppression, scheduler admission, restart persistence, optimistic
  conflicts, and every historical schema upgrade path. Production worker
  orchestration remains open before the subscription task can be checked.
- 2026-07-12: Accepted ADR-0027 and made admission state evolve within each
  scheduler pass. SQLite contracts prove same-tick single and parallel bounds,
  FIFO queued deferral and overflow suppression, next-run admission after a
  terminal predecessor, and restart cancellation of the prior run, trace, and
  timer before replacement intent.
