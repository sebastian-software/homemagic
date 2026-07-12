---
id: E4-008-04
epic: EPIC-004
parent: E4-008
title: Score candidates and accept controller selection ADR
status: planned
priority: critical
depends_on: [E4-008-03]
adrs: [ADR-0005, ADR-0038, ADR-0039]
created: 2026-07-12
updated: 2026-07-12
---

# E4-008-04: Selection ADR

## Outcome

The committed evidence matrix either selects one passing candidate in ADR-0039
with replacement triggers, or records a scoped blocker without weakening gates.

## Verification

- [ ] Every mandatory gate and weighted score links to captured evidence.
- [ ] Tie-breaks and non-Rust exceptions follow the predeclared rules.
- [ ] Rejected/reference-only candidates remain outside production manifests.
