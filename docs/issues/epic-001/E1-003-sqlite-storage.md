---
id: E1-003
epic: EPIC-001
title: Implement SQLite storage and repository contracts
status: done
priority: critical
depends_on: [E1-001, E1-002]
adrs: []
created: 2026-07-11
updated: 2026-07-11
---

# E1-003: SQLite Storage

## Outcome

`homemagic-storage` provides transactional, migration-backed repositories for
the complete EPIC-001 domain model without leaking SQL into application or
integration crates.

## Tasks

- [x] Add the workspace crate and implement application repository traits.
  Evidence: `crates/homemagic-storage` and `FoundationRepository`.
- [x] Enable WAL, foreign keys, busy timeout, and explicit migrations. Evidence:
  `open_connection`, ADR-0007-compatible migration ledger, and health tests.
- [x] Persist installations, integrations, devices, endpoints, capabilities,
  aliases, spaces, observations, diagnostics, repairs, and metadata. Evidence:
  `repository_should_persist_every_foundation_projection`.
- [x] Enforce native identity uniqueness within an integration instance.
  Evidence: schema constraint and `native_identity_collision_should_roll_back_all_devices`.
- [x] Implement transactional upsert and reconciliation. Evidence:
  `FoundationRepository::apply` and rollback contract tests.
- [x] Expose schema version and database health. Evidence: `StorageHealth` and
  `repository_should_report_schema_and_wal_health`.
- [x] Implement online backup and validated restore helpers. Evidence:
  `backup.rs` and `backup_restore.rs`.
- [x] Commit a schema fixture for every released migration version. Evidence:
  `tests/fixtures/schema-v0.sql`, `schema-v1-seed.sql`, and fixture tests.

## Acceptance criteria

- [x] Repository contract tests pass against an isolated database. Evidence:
  every storage integration test uses `tempfile::TempDir`.
- [x] Restart preserves device, endpoint, and capability IDs. Evidence:
  `repository_should_preserve_stable_device_id_across_reopen` compares the full
  `DeviceRecord` after reopening the database.
- [x] A failed reconciliation leaves no partial write. Evidence:
  `failed_write_should_roll_back_every_prior_row` and the identity collision test.
- [x] Every historical fixture upgrades to the current schema. Evidence:
  `migration_fixtures.rs` covers schema versions 0 and 1.
- [x] Backup restored into a new location passes integrity and repository checks.
  Evidence: `backup_and_restore_should_preserve_foundation_data`.

## Verification

- [x] `cargo test -p homemagic-storage --all-features --locked`
- [x] Migration fixture test from every committed version. Evidence:
  `migration_fixtures.rs`.
- [x] Backup/restore integration test. Evidence: `backup_restore.rs`, including
  the non-destructive invalid-restore case.
- [x] Foreign-key and native-identity constraint tests. Evidence:
  `transaction_contract.rs`.

## Progress log

- 2026-07-11: Issue created.
- 2026-07-11: Started the storage crate, initial migration, normalized schema,
  domain mappings, and repository contract tests.
- 2026-07-11: Completed schema v1, repository contracts, historical fixtures,
  backup/restore, health, atomicity, and constraint verification. Full workspace
  format, Clippy, tests, and doctests pass.
