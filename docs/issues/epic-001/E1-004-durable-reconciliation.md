---
id: E1-004
epic: EPIC-001
title: Load durable state and reconcile discovery candidates
status: planned
priority: critical
depends_on: [E1-003]
adrs: [ADR-0006]
created: 2026-07-11
updated: 2026-07-11
---

# E1-004: Durable Reconciliation

## Outcome

Known devices are readable immediately after startup and discovery converges
them with the network without replacing or deleting durable identity.

## Tasks

- [ ] Load persisted state before starting network reconciliation.
- [ ] Separate discovery candidates from enrolled devices.
- [ ] Reconcile by integration instance and native identifier.
- [ ] Preserve devices missed by a bounded discovery window.
- [ ] Handle network-address and mutable-metadata changes.
- [ ] Detect identity collisions and create repair records.
- [ ] Define explicit removal and rediscovery operations.
- [ ] Publish causally linked reconciliation and lifecycle events.

## Acceptance criteria

- [ ] Reads return persisted devices before the startup scan completes.
- [ ] A discovery miss never removes or renumbers a known device.
- [ ] Rediscovery after address change retains all stable IDs.
- [ ] A collision prevents merging and exposes one actionable repair.
- [ ] Reconciliation is idempotent for identical input.

## Verification

- [ ] Restart integration test with stable IDs.
- [ ] Miss, address-change, collision, removal, and rediscovery tests.
- [ ] Transaction rollback test for a failed reconciliation.

## Progress log

- 2026-07-11: Issue created.
