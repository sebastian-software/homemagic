---
id: E1-003
epic: EPIC-001
title: Implement SQLite storage and repository contracts
status: ready
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

- [ ] Add the workspace crate and implement application repository traits.
- [ ] Enable WAL, foreign keys, busy timeout, and explicit migrations.
- [ ] Persist installations, integrations, devices, endpoints, capabilities,
      aliases, spaces, observations, diagnostics, repairs, and metadata.
- [ ] Enforce native identity uniqueness within an integration instance.
- [ ] Implement transactional upsert and reconciliation.
- [ ] Expose schema version and database health.
- [ ] Implement online backup and validated restore helpers.
- [ ] Commit a schema fixture for every released migration version.

## Acceptance criteria

- [ ] Repository contract tests pass against an isolated database.
- [ ] Restart preserves device, endpoint, and capability IDs.
- [ ] A failed reconciliation leaves no partial write.
- [ ] Every historical fixture upgrades to the current schema.
- [ ] Backup restored into a new location passes integrity and repository checks.

## Verification

- [ ] `cargo test -p homemagic-storage`
- [ ] Migration fixture test from every committed version.
- [ ] Backup/restore integration test.
- [ ] Foreign-key and native-identity constraint tests.

## Progress log

- 2026-07-11: Issue created.
