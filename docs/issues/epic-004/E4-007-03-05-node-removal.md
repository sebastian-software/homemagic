---
id: E4-007-03-05
epic: EPIC-004
parent: E4-007-03
title: Remove nodes without hiding partial cleanup
status: in_progress
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

- [x] Admit removal only for a durable node in the actor installation.
- [x] Transition to `removing_node` before the controller call.
- [x] Reconcile removed, already-absent, and partial controller outcomes.
- [x] Transition to `cleaning_secrets` before owned metadata cleanup.
- [x] Tombstone or retain node/projection/subscription facts atomically according
  to the proven cleanup outcome.
- [x] Preserve partial cleanup with structured repair facts.
- [x] Recover removal restart checkpoints without blind redispatch.

## Acceptance criteria

- [x] Successful removal leaves no active common capability for the node.
- [x] Already-absent controller state can complete idempotently.
- [x] Partial or unknown outcomes keep enough node metadata for repair.
- [x] Duplicate requests never issue duplicate unproven physical work.

## Verification

- [x] Success, absent, partial, duplicate, conflict, foreign, atomic rollback,
  and reopen tests pass.
- [x] Restart at `removing_node` and `cleaning_secrets` reaches an explicit
  terminal outcome.
- [x] Partial cleanup remains listable after database reopen.
- [x] Strict workspace Clippy and targeted removal contracts pass locally.
- [x] Full local workspace, migration, boundary, and secret-scan gates pass.
- [ ] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: E4-007-03-04 completed with public cross-platform CI. This child
  issue is ready.
- 2026-07-12: Implemented actor-bound idempotent removal, explicit absent and
  partial reconciliation, atomic common-device tombstoning, retained repair
  metadata, and bounded restart recovery. Six focused removal contracts and
  strict workspace Clippy pass.
- 2026-07-12: All 44 Matter repository contracts, the complete all-feature
  workspace suite, strict Clippy, Matter boundary checks, and secret scans pass
  locally. Commit, push, and public CI remain pending.
