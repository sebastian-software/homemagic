---
id: E4-007-03-02
epic: EPIC-004
parent: E4-007-03
title: Commit commissioned nodes and capability projections atomically
status: planned
priority: high
depends_on: [E4-007-03-01]
adrs: [ADR-0033, ADR-0034, ADR-0037, ADR-0040]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-03-02: Commissioning Projection

## Outcome

Controller commissioning progress becomes durable application phases, and
success atomically exposes the authoritative node, common capability
projections, initial logical subscriptions, operation-result link, and completed
operation.

## Tasks

- [ ] Transition to `validating_setup` before crossing the controller boundary.
- [ ] Reconcile bounded controller progress events in declared phase order.
- [ ] Project the returned descriptor through the accepted capability rules.
- [ ] Materialize reference-only initial projection and subscription rows.
- [ ] Commit node, projections, subscriptions, result link, and completion in
  one repository transaction.
- [ ] Reject mismatched fabric, duplicate node, and malformed descriptor results.

## Acceptance criteria

- [ ] A completed operation always resolves to one queryable stored node.
- [ ] No node or capability becomes visible before the atomic success commit.
- [ ] Simulator light and lock use the same common capability projection rules.
- [ ] Controller progress cannot skip or reorder durable domain phases.

## Verification

- [ ] Light and lock commissioning/reopen/projection contracts pass.
- [ ] Atomic-failure tests expose neither partial nodes nor partial projections.
- [ ] Duplicate controller results cannot create duplicate common identities.
