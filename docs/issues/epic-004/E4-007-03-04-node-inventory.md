---
id: E4-007-03-04
epic: EPIC-004
parent: E4-007-03
title: Expose authenticated bounded durable node inventory
status: in_progress
priority: high
depends_on: [E4-007-03-02]
adrs: [ADR-0013, ADR-0033, ADR-0034]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-03-04: Node Inventory

## Outcome

Authenticated installation readers can list and inspect bounded durable Matter
node metadata and operation result links without exposing secret references or
raw controller objects.

## Tasks

- [x] Add repository get/list methods scoped by installation and fabric.
- [x] Define secret-free bounded node summary and detail DTOs.
- [x] Expose stable device/projection/subscription identities with descriptors.
- [x] Revalidate current `matter_read` authority for every request.
- [x] Return newest descriptor revisions deterministically.

## Acceptance criteria

- [x] Cross-installation node reads return no existence oracle.
- [x] Lists are bounded, deterministic, and stable across reopen.
- [x] DTOs contain no secret references or SDK-specific types.

## Verification

- [x] Empty, populated, bounded, foreign, disabled-actor, and reopen tests pass.
- [x] Operation-to-node result lookup survives restart.
- [x] Strict Clippy and the targeted inventory contract pass locally.
- [x] Full local workspace, migration, boundary, and secret-scan gates pass.
- [ ] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: E4-007-03-03 completed with public cross-platform CI. This child
  issue is ready.
- 2026-07-12: Implemented authenticated bounded durable inventory summaries and
  details, deterministic relational loading, installation isolation, current
  read-grant revalidation, and restart-stable operation links. Targeted tests
  and strict Clippy passed before the complete local gate.
- 2026-07-12: All 38 Matter repository contracts, the complete all-feature
  workspace suite, strict Clippy, Matter boundary checks, and secret scans pass
  locally. Commit, push, and public CI remain pending.
