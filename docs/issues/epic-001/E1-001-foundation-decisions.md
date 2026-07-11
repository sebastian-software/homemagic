---
id: E1-001
epic: EPIC-001
title: Decide persistence, secret storage, and event retention
status: done
priority: critical
depends_on: []
adrs: [ADR-0007, ADR-0008, ADR-0009]
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

- [x] Add and index the SQLite ownership and migration ADR. Evidence: ADR-0007.
- [x] Add and index the cross-platform secret-storage ADR. Evidence: ADR-0008.
- [x] Add and index the event-retention and snapshot ADR. Evidence: ADR-0009.
- [x] Confirm each decision preserves the 95%+ Rust and selective-FFI policy.
  Evidence: the FFI reviews in ADR-0007 and ADR-0008; ADR-0009 adds no FFI.
- [x] Link all three decisions from EPIC-001. Evidence: EPIC-001 required
  decisions and `docs/adr/README.md`.

## Acceptance criteria

- [x] A migration can be classified as compatible, destructive, or unsupported.
  Evidence: ADR-0007, Migration policy.
- [x] Backup and restore behavior has a testable consistency contract. Evidence:
  ADR-0007, Backup and restore contract.
- [x] No device snapshot or API needs access to plaintext credentials. Evidence:
  ADR-0008, Decision and Secret lifecycle and redaction.
- [x] Headless Linux startup has a documented secure configuration path.
  Evidence: ADR-0008, Headless key provisioning.
- [x] Current state remains available even after historical events expire.
  Evidence: ADR-0009, Current state.

## Verification

- [x] ADR index links resolve. Evidence: `docs/adr/README.md` and repository link
  verification.
- [x] ADRs contain no unresolved placeholders. Evidence: placeholder scan over
  `docs/adr`.
- [x] Cross-ADR terminology is consistent with ADR-0001 through ADR-0006.
  Evidence: ADR self-review and ADR-0005 exception sections.

## Progress log

- 2026-07-11: Issue created.
- 2026-07-11: Completed and accepted ADR-0007 through ADR-0009; E1-002 is now
  unblocked.
