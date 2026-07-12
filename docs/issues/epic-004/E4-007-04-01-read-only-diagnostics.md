---
id: E4-007-04-01
epic: EPIC-004
parent: E4-007-04
title: Expose bounded read-only Matter diagnostics
status: in_progress
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

- [x] Define versioned diagnostic summary/detail DTOs.
- [x] Add bounded repository reads for operations and open repairs by resource.
- [x] Join durable node inventory with at most one bounded controller snapshot.
- [x] Redact native network, setup, secret-reference, and SDK-specific fields.
- [x] Revalidate current `matter_read` authority on every request.

## Acceptance criteria

- [x] Repeated diagnostics produce no durable writes or controller mutation.
- [x] Foreign resources follow the same missing path as absent resources.
- [x] Ordering and page bounds remain stable after reopen.

## Verification

- [x] Empty, populated, bounded, foreign, disabled-actor, and reopen tests pass.
- [x] Controller call counting proves diagnostics are read-only and bounded.
- [x] Diagnostic JSON passes setup, secret, native-address, and SDK canaries.
- [x] Full local workspace, strict Clippy, boundary, and secret-scan gates pass.
- [ ] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: Implemented `matter.diagnostics.v1` as a read-only authenticated
  bounded snapshot with redacted controller, fabric, common-node, endpoint,
  subscription, actor-operation, and repair health. All 45 Matter repository
  contracts and complete local CI-equivalent gates pass. Controller call-count
  evidence confirms one bounded status read per diagnostic snapshot and zero
  controller mutations. Commit, push, and public CI remain pending.
