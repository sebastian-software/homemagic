---
id: E4-007-03
epic: EPIC-004
parent: E4-007
title: Orchestrate commissioning and node removal durably
status: ready
priority: high
depends_on: [E4-007-01, E4-007-02]
adrs: [ADR-0033, ADR-0034, ADR-0037]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-03: Node Operation Workflows

## Outcome

Commissioning and removal execute as durable phase machines that survive every
simulator restart point, support bounded cancellation, and leave partial cleanup
visible and repairable.

## Tasks

- [ ] Implement commissioning start, get, list, and eligible cancellation.
- [ ] Consume setup payload only at the sensitive controller boundary.
- [ ] Persist node identity and projected capabilities before completion.
- [ ] Implement node list/get and removal orchestration.
- [ ] Resume or classify every nonterminal phase on restart.
- [ ] Record partial cleanup as repair-required instead of hiding the node.

## Acceptance criteria

- [ ] Every simulated lifecycle restart ends completed, failed, cancelled, or
  repair-required.
- [ ] Setup codes never enter durable ordinary fields.
- [ ] Removal cannot silently discard unresolved fabric or projection state.

## Verification

- [ ] Phase-by-phase restart and cancellation matrices pass.
- [ ] Duplicate commissioning and removal requests remain idempotent.
- [ ] Partial cleanup stays queryable after reopen.

## Progress log

- 2026-07-12: E4-007-02 completed with public cross-platform CI. This issue is
  ready for decomposition before implementation.
