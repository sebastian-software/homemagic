---
id: E4-006-02
epic: EPIC-004
parent: E4-006
title: Wire desired-state supersession into the shared command path
status: done
priority: critical
depends_on: [E4-006-01]
adrs: [ADR-0014, ADR-0036]
created: 2026-07-12
updated: 2026-07-12
---

# E4-006-02: Desired-State Supersession

## Outcome

Every eligible state-setting command receives a durable monotonic desired
revision, and a newer command atomically cancels only the older command that has
not crossed the durable dispatched boundary.

## Tasks

- [x] Resolve common command targets to their stable Matter projection.
- [x] Expose the current durable desired slot through the repository contract.
- [x] Register each eligible command after receipt and before dispatch.
- [x] Atomically update the slot, cancel the old command, append its audit, and
  link the supersession.
- [x] Publish the committed cancellation audit after the transaction.
- [x] Keep toggle, stop, pulse, and other intermediate-effect commands outside
  replaceable-state admission unless explicitly reduced beforehand.
- [x] Preserve an already dispatched command as immutable history when a newer
  desired revision takes ownership of the slot.
- [x] Restore slot coordination deterministically after daemon restart.

## Acceptance criteria

- [x] `on -> off -> on` with paused dispatch admits only final `on` at the
  adapter boundary.
- [x] Every request retains its command and audit history.
- [x] A dispatched intermediate command is never cancelled or rewritten.
- [x] A newer lock invalidates a still-undispatched unlock and its unused
  authorization facts.

## Verification

- [x] Pre-dispatch boundary and concurrent registration tests pass.
- [x] Repository rollback leaves command, audit, slot, and link unchanged.
- [x] Restart tests preserve the latest desired revision and command identity.

## Progress log

- 2026-07-12: E4-006-01 completed typed replaceable access-control commands;
  this issue is ready.
- 2026-07-12: Wired optimistic desired-state registration and atomic dispatch
  admission into the shared command service. Serial, concurrent, rollback,
  post-dispatch-history, and reopen tests pass. E4-006-02 is done.
