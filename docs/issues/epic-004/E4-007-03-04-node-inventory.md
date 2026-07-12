---
id: E4-007-03-04
epic: EPIC-004
parent: E4-007-03
title: Expose authenticated bounded durable node inventory
status: ready
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

- [ ] Add repository get/list methods scoped by installation and fabric.
- [ ] Define secret-free bounded node summary and detail DTOs.
- [ ] Expose stable device/projection/subscription identities with descriptors.
- [ ] Revalidate current `matter_read` authority for every request.
- [ ] Return newest descriptor revisions deterministically.

## Acceptance criteria

- [ ] Cross-installation node reads return no existence oracle.
- [ ] Lists are bounded, deterministic, and stable across reopen.
- [ ] DTOs contain no secret references or SDK-specific types.

## Verification

- [ ] Empty, populated, bounded, foreign, disabled-actor, and reopen tests pass.
- [ ] Operation-to-node result lookup survives restart.

## Progress log

- 2026-07-12: E4-007-03-03 completed with public cross-platform CI. This child
  issue is ready.
