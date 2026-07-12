---
id: E4-007-04-01
epic: EPIC-004
parent: E4-007-04
title: Expose bounded read-only Matter diagnostics
status: ready
priority: high
depends_on: [E4-007-03]
adrs: [ADR-0013, ADR-0033, ADR-0034, ADR-0041]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-04-01: Read-only Diagnostics

## Outcome

Authenticated installation readers can inspect bounded, deterministic,
secret-free fabric, node, endpoint, projection, operation, repair, and
controller evidence without triggering any mutation.

## Tasks

- [ ] Define versioned diagnostic summary/detail DTOs.
- [ ] Add bounded repository reads for operations and open repairs by resource.
- [ ] Join durable node inventory with at most one bounded controller snapshot.
- [ ] Redact native network, setup, secret-reference, and SDK-specific fields.
- [ ] Revalidate current `matter_read` authority on every request.

## Acceptance criteria

- [ ] Repeated diagnostics produce no durable writes or controller mutation.
- [ ] Foreign resources follow the same missing path as absent resources.
- [ ] Ordering and page bounds remain stable after reopen.

## Verification

- [ ] Empty, populated, bounded, foreign, disabled-actor, and reopen tests pass.
- [ ] Controller call counting proves diagnostics are read-only and bounded.
- [ ] Diagnostic JSON passes setup, secret, native-address, and SDK canaries.
