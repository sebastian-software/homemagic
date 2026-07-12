---
id: E4-007-03
epic: EPIC-004
parent: E4-007
title: Orchestrate commissioning and node removal durably
status: in_progress
priority: high
depends_on: [E4-007-01, E4-007-02]
adrs: [ADR-0033, ADR-0034, ADR-0037, ADR-0040]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-03: Node Operation Workflows

## Child issues

| Issue | Status | Outcome |
| --- | --- | --- |
| [E4-007-03-01](E4-007-03-01-commissioning-target-admission.md) | In progress | Fabric-scoped commissioning admission and sensitive input boundary |
| [E4-007-03-02](E4-007-03-02-commissioning-projection.md) | Planned | Atomic node, projection, subscription, and operation-result commit |
| [E4-007-03-03](E4-007-03-03-cancellation-recovery.md) | Planned | Best-effort cancellation and phase-by-phase restart reconciliation |
| [E4-007-03-04](E4-007-03-04-node-inventory.md) | Planned | Authenticated bounded durable node inventory |
| [E4-007-03-05](E4-007-03-05-node-removal.md) | Planned | Idempotent removal with visible partial cleanup |

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
- 2026-07-12: Decomposed into five dependency-ordered slices. ADR-0040 resolves
  the pre-commissioning identity gap without inventing a node ID; E4-007-03-01
  is ready.
- 2026-07-12: E4-007-03-01 is implemented and locally verified. Commit, push,
  public CI, and child closure remain pending.
