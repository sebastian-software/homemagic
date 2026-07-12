---
id: E4-008-04
epic: EPIC-004
parent: E4-008
title: Score candidates and accept controller selection ADR
status: in_progress
priority: critical
depends_on: [E4-008-03]
adrs: [ADR-0005, ADR-0038, ADR-0039]
created: 2026-07-12
updated: 2026-07-12
---

# E4-008-04: Selection ADR

## Child issues

| Issue | Status | Outcome |
| --- | --- | --- |
| [E4-008-04-01](E4-008-04-01-connectedhomeip-contingency.md) | Ready | Official SDK narrow-boundary audit |
| [E4-008-04-02](E4-008-04-02-matter-js-contingency.md) | In progress | Isolated sidecar fallback audit |
| [E4-008-04-03](E4-008-04-03-final-matrix-adr.md) | Planned | Frozen matrix and ADR-0039 |

## Outcome

The committed evidence matrix either selects one passing candidate in ADR-0039
with replacement triggers, or records a scoped blocker without weakening gates.

## Verification

- [ ] Every mandatory gate and weighted score links to captured evidence.
- [ ] Tie-breaks and non-Rust exceptions follow the predeclared rules.
- [ ] Rejected/reference-only candidates remain outside production manifests.

## Progress log

- 2026-07-12: Both Rust-native candidates fail mandatory gates before weighted
  scoring. ADR-0038 therefore activates the predeclared non-Rust contingencies.
  The official C++ SDK is evaluated first because the product preference is a
  narrow FFI/process exception before a broader non-Rust sidecar. `matter.js`
  remains the independently isolated fallback; neither may bypass ADR-0005.
