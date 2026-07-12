---
id: E4-008-03
epic: EPIC-004
parent: E4-008
title: Run controller contract and independent reference lifecycle
status: done
priority: critical
depends_on: [E4-008-02]
adrs: [ADR-0033, ADR-0038]
created: 2026-07-12
updated: 2026-07-12
---

# E4-008-03: Contract and Reference Evidence

## Outcome

The surviving candidate is adapted only inside a spike and runs the fixed
`MatterController` contract plus the on-network lifecycle against an independent
reference implementation, with unsupported cases recorded explicitly.

## Verification

- [x] Candidate self-tests and independent-reference results remain distinct.
- [x] Fabric, commission, inventory, read, invoke, subscribe, restart, and remove
  outcomes are recorded per host.
- [x] Failure normalization and cancellation gaps are explicit.

## Progress log

- 2026-07-12: The isolated `rust-matc` spike compiles against the exact pin and
  maps the fixed lifecycle without entering production manifests. Static source
  analysis found missing Device Attestation verification and no commissioning
  cancellation handle in both native candidates; these remain mandatory gaps,
  not warnings. The independent `rs-matter` device lifecycle is running locally
  before the same script is promoted to the two-host workflow.
- 2026-07-12: The fresh, mDNS-free independent `rs-matter` fixture received
  `ArmFailSafe`, but `rust-matc` did not complete commissioning within the
  bounded run. The spike now emits phase-specific partial outcomes and the
  last independently observed protocol step; a two-host evidence workflow is
  prepared. No downstream lifecycle result is claimed from this failed gate.
- 2026-07-12: Public workflow run `29211713067` captured both hosts. Linux
  timed out in commissioning after the independent fixture observed
  `ArmFailSafe`. macOS passed fabric creation, commissioning, inventory, read,
  and subscription establishment, then failed invoke; restart and removal were
  not run. Both reports are committed, so this evidence issue is done with a
  failed mandatory lifecycle gate rather than a selected candidate.
