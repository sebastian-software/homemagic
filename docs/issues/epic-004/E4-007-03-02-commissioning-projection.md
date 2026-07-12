---
id: E4-007-03-02
epic: EPIC-004
parent: E4-007-03
title: Commit commissioned nodes and capability projections atomically
status: in_progress
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

- [x] Transition to `validating_setup` before crossing the controller boundary.
- [x] Reconcile bounded controller progress events in declared phase order.
- [x] Project the returned descriptor through the accepted capability rules.
- [x] Materialize reference-only initial projection and subscription rows.
- [x] Commit node, projections, subscriptions, result link, and completion in
  one repository transaction.
- [x] Reject mismatched fabric, duplicate node, and malformed descriptor results.

## Acceptance criteria

- [x] A completed operation always resolves to one queryable stored node.
- [x] No node or capability becomes visible before the atomic success commit.
- [x] Simulator light and lock use the same common capability projection rules.
- [x] Controller progress cannot skip or reorder durable domain phases.

## Verification

- [x] Light and lock commissioning/reopen/projection contracts pass.
- [x] Atomic-failure tests expose neither partial nodes nor partial projections.
- [x] Duplicate controller results cannot create duplicate common identities.
- [x] Full local workspace gates pass.
- [ ] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: E4-007-03-01 completed with public cross-platform CI. This child
  issue is ready.
- 2026-07-12: Implemented exact controller-phase reconciliation, bounded initial
  state reads, shared light/lock projection, logical subscription, and one
  atomic integration/device/node/projection/subscription/result/completion
  commit. Thirty-one Matter repository contracts, full workspace tests, strict
  Clippy, boundary checks, and secret scans pass locally. Commit, push, and
  public CI remain pending.
