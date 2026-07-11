---
id: E1-004
epic: EPIC-001
title: Load durable state and reconcile discovery candidates
status: done
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

- [x] Load persisted state before starting network reconciliation. Evidence:
  `HomeMagicApplication::from_repository`, `durable_application`, and
  `durable_state_should_be_readable_before_discovery`.
- [x] Separate discovery candidates from enrolled devices. Evidence:
  `DiscoveryCandidate`, `DeviceRecord`, and the updated `IntegrationScanner`.
- [x] Reconcile by integration instance and native identifier. Evidence:
  `reconciliation.rs` and stable integration-instance identity tests.
- [x] Preserve devices missed by a bounded discovery window. Evidence:
  `discovery_miss_should_not_change_known_device`.
- [x] Handle network-address and mutable-metadata changes. Evidence:
  `rediscovery_should_update_mutable_state_and_preserve_identity`.
- [x] Detect identity collisions and create repair records. Evidence:
  `mismatched_native_identity_should_create_repair_without_merge`.
- [x] Define explicit removal and rediscovery operations. Evidence:
  `HomeMagicApplication::remove_device`, tombstone persistence, and rediscovery
  tests.
- [x] Publish causally linked reconciliation and lifecycle events. Evidence:
  `refresh_should_publish_correlated_typed_events`.

## Acceptance criteria

- [x] Reads return persisted devices before the startup scan completes. Evidence:
  load-first application test and background initial refresh in `serve`.
- [x] A discovery miss never removes or renumbers a known device. Evidence:
  discovery-miss reconciliation test.
- [x] Rediscovery after address change retains all stable IDs. Evidence:
  mutable-state reconciliation test and `DeviceRecord::replace_snapshot`.
- [x] A collision prevents merging and exposes one actionable repair. Evidence:
  collision repair test.
- [x] Reconciliation is idempotent for identical input. Evidence:
  `identical_candidate_should_be_idempotent`.

## Verification

- [x] Restart integration test with stable IDs. Evidence: storage reopen contract
  plus `bootstrap_should_reuse_identities_after_reopen`.
- [x] Miss, address-change, collision, removal, and rediscovery tests. Evidence:
  eight Application/Reconciliation contract tests.
- [x] Transaction rollback test for a failed reconciliation. Evidence:
  `crates/homemagic-storage/tests/transaction_contract.rs`.

## Progress log

- 2026-07-11: Issue created.
- 2026-07-11: Started discovery-candidate contracts, durable registry loading,
  reconciliation, lifecycle events, collision repairs, and explicit removal.
- 2026-07-11: Completed load-first daemon composition, durable reconciliation,
  explicit removal/rediscovery, collision repairs, and causal event fan-out.
  Evidence: full workspace format, Clippy, tests, doctests, and local daemon
  `/health` plus `devices.list` smoke tests.
