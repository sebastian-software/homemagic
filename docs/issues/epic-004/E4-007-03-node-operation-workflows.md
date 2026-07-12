---
id: E4-007-03
epic: EPIC-004
parent: E4-007
title: Orchestrate commissioning and node removal durably
status: done
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
| [E4-007-03-01](E4-007-03-01-commissioning-target-admission.md) | Done | Fabric-scoped commissioning admission and sensitive input boundary |
| [E4-007-03-02](E4-007-03-02-commissioning-projection.md) | Done | Atomic node, projection, subscription, and operation-result commit |
| [E4-007-03-03](E4-007-03-03-cancellation-recovery.md) | Done | Best-effort cancellation and phase-by-phase restart reconciliation |
| [E4-007-03-04](E4-007-03-04-node-inventory.md) | Done | Authenticated bounded durable node inventory |
| [E4-007-03-05](E4-007-03-05-node-removal.md) | Done | Idempotent removal with visible partial cleanup |

## Outcome

Commissioning and removal execute as durable phase machines that survive every
simulator restart point, support bounded cancellation, and leave partial cleanup
visible and repairable.

## Tasks

- [x] Implement commissioning start, get, list, and eligible cancellation.
- [x] Consume setup payload only at the sensitive controller boundary.
- [x] Persist node identity and projected capabilities before completion.
- [x] Implement node list/get and removal orchestration.
- [x] Resume or classify every nonterminal phase on restart.
- [x] Record partial cleanup as repair-required instead of hiding the node.

## Acceptance criteria

- [x] Every simulated lifecycle restart ends completed, failed, cancelled, or
  repair-required.
- [x] Setup codes never enter durable ordinary fields.
- [x] Removal cannot silently discard unresolved fabric or projection state.

## Verification

- [x] Phase-by-phase restart and cancellation matrices pass.
- [x] Duplicate commissioning and removal requests remain idempotent.
- [x] Partial cleanup stays queryable after reopen.

## Progress log

- 2026-07-12: E4-007-02 completed with public cross-platform CI. This issue is
  ready for decomposition before implementation.
- 2026-07-12: Decomposed into five dependency-ordered slices. ADR-0040 resolves
  the pre-commissioning identity gap without inventing a node ID; E4-007-03-01
  is ready.
- 2026-07-12: E4-007-03-01 is implemented and locally verified. Commit, push,
  public CI, and child closure remain pending.
- 2026-07-12: Public CI run `29203093982` verified E4-007-03-01 on Linux x86_64
  and macOS ARM64. E4-007-03-01 is done and E4-007-03-02 is ready.
- 2026-07-12: E4-007-03-02 is implemented and locally verified. Commit, push,
  public CI, and child closure remain pending.
- 2026-07-12: Public CI run `29203595736` verified E4-007-03-02 on Linux x86_64
  and macOS ARM64. E4-007-03-02 is done and E4-007-03-03 is ready.
- 2026-07-12: E4-007-03-03 implements local and in-flight cancellation, atomic
  original/cancellation reconciliation, foreign-operation isolation, and
  fail-closed bounded recovery across every simulator checkpoint. Local gates
  pass; commit, push, and public CI remain pending.
- 2026-07-12: Public CI run `29204270373` verified E4-007-03-03 on Linux x86_64
  and macOS ARM64. E4-007-03-03 is done and E4-007-03-04 is ready.
- 2026-07-12: E4-007-03-04 authenticated bounded durable node inventory is
  implemented. All local CI-equivalent gates pass; commit, push, and public CI
  remain pending.
- 2026-07-12: Public CI run `29204953299` verified E4-007-03-04 on Linux x86_64
  and macOS ARM64. E4-007-03-04 is done and E4-007-03-05 is ready.
- 2026-07-12: E4-007-03-05 node removal, atomic tombstoning, partial repair,
  and restart recovery are implemented. All local CI-equivalent gates pass;
  commit, push, and public CI remain pending.
- 2026-07-12: Public CI run `29205464608` verified E4-007-03-05 on Linux x86_64
  and macOS ARM64. All five child issues are done; this parent issue is done.
