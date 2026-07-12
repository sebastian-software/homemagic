---
id: E4-008-03
epic: EPIC-004
parent: E4-008
title: Run controller contract and independent reference lifecycle
status: ready
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

- [ ] Candidate self-tests and independent-reference results remain distinct.
- [ ] Fabric, commission, inventory, read, invoke, subscribe, restart, and remove
  outcomes are recorded per host.
- [ ] Failure normalization and cancellation gaps are explicit.
