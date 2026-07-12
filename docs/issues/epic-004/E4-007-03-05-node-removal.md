---
id: E4-007-03-05
epic: EPIC-004
parent: E4-007-03
title: Remove nodes without hiding partial cleanup
status: ready
priority: high
depends_on: [E4-007-03-03, E4-007-03-04]
adrs: [ADR-0014, ADR-0033, ADR-0037, ADR-0040]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-03-05: Node Removal

## Outcome

Node removal is an actor-bound idempotent phase machine. Controller removal,
HomeMagic projection cleanup, and owned secret cleanup are distinguished so an
incomplete outcome stays queryable and repairable.

## Tasks

- [ ] Admit removal only for a durable node in the actor installation.
- [ ] Transition to `removing_node` before the controller call.
- [ ] Reconcile removed, already-absent, and partial controller outcomes.
- [ ] Transition to `cleaning_secrets` before owned metadata cleanup.
- [ ] Tombstone or retain node/projection/subscription facts atomically according
  to the proven cleanup outcome.
- [ ] Preserve partial cleanup with structured repair facts.
- [ ] Recover removal restart checkpoints without blind redispatch.

## Acceptance criteria

- [ ] Successful removal leaves no active common capability for the node.
- [ ] Already-absent controller state can complete idempotently.
- [ ] Partial or unknown outcomes keep enough node metadata for repair.
- [ ] Duplicate requests never issue duplicate unproven physical work.

## Verification

- [ ] Success, absent, partial, duplicate, conflict, and reopen tests pass.
- [ ] Restart at `removing_node` and `cleaning_secrets` reaches an explicit
  terminal outcome.
- [ ] Partial cleanup remains listable after database reopen.

## Progress log

- 2026-07-12: E4-007-03-04 completed with public cross-platform CI. This child
  issue is ready.
