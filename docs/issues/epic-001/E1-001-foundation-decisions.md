---
id: E1-001
epic: EPIC-001
title: Decide persistence, secret storage, and event retention
status: ready
priority: critical
depends_on: []
adrs: []
created: 2026-07-11
updated: 2026-07-11
---

# E1-001: Foundation Decisions

## Outcome

The storage, migration, backup, secret-store, and event-retention contracts are
explicit enough that later implementation does not embed accidental policy.

## Scope

- Define SQLite schema ownership and compatibility guarantees.
- Define forward-only migration and historical-fixture policy.
- Define consistent online backup and validated restore behavior.
- Select macOS and Linux secret-store adapters and a headless fallback.
- Define secret-reference lifecycle and redaction boundaries.
- Define retention separately for current snapshots and immutable events.

## Tasks

- [ ] Add and index the SQLite ownership and migration ADR.
- [ ] Add and index the cross-platform secret-storage ADR.
- [ ] Add and index the event-retention and snapshot ADR.
- [ ] Confirm each decision preserves the 95%+ Rust and selective-FFI policy.
- [ ] Link all three decisions from EPIC-001.

## Acceptance criteria

- [ ] A migration can be classified as compatible, destructive, or unsupported.
- [ ] Backup and restore behavior has a testable consistency contract.
- [ ] No device snapshot or API needs access to plaintext credentials.
- [ ] Headless Linux startup has a documented secure configuration path.
- [ ] Current state remains available even after historical events expire.

## Verification

- [ ] ADR index links resolve.
- [ ] ADRs contain no unresolved placeholders.
- [ ] Cross-ADR terminology is consistent with ADR-0001 through ADR-0006.

## Progress log

- 2026-07-11: Issue created.
