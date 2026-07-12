---
id: E4-006-02
epic: EPIC-004
parent: E4-006
title: Wire desired-state supersession into the shared command path
status: ready
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

- [ ] Resolve common command targets to their stable Matter projection.
- [ ] Expose the current durable desired slot through the repository contract.
- [ ] Register each eligible command after receipt and before dispatch.
- [ ] Atomically update the slot, cancel the old command, append its audit, and
  link the supersession.
- [ ] Publish the committed cancellation audit after the transaction.
- [ ] Keep toggle, stop, pulse, and other intermediate-effect commands outside
  replaceable-state admission unless explicitly reduced beforehand.
- [ ] Reject attempts to replace a slot whose command is already dispatched.
- [ ] Restore slot coordination deterministically after daemon restart.

## Acceptance criteria

- [ ] `on -> off -> on` with paused dispatch invokes only final `on`.
- [ ] Every request retains its command and audit history.
- [ ] A dispatched intermediate command is never cancelled or rewritten.
- [ ] A newer lock invalidates a still-undispatched unlock and its unused
  authorization facts.

## Verification

- [ ] Pre-dispatch barrier and concurrent registration tests pass.
- [ ] Repository rollback leaves command, audit, slot, and link unchanged.
- [ ] Restart tests preserve the latest desired revision and command identity.

## Progress log

- 2026-07-12: E4-006-01 completed typed replaceable access-control commands;
  this issue is ready.
