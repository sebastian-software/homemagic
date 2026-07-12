---
id: E4-008-04-03
epic: EPIC-004
parent: E4-008-04
title: Commit the final controller matrix and ADR-0039
status: done
priority: critical
depends_on: [E4-008-04-01, E4-008-04-02]
adrs: [ADR-0005, ADR-0038, ADR-0039]
created: 2026-07-12
updated: 2026-07-12
---

# E4-008-04-03: Final Matrix and ADR

## Outcome

The frozen matrix selects only a candidate that passes every mandatory gate, or
ADR-0039 records the precise blocker and executable remediation without
weakening the contract.

## Verification

- [x] Failed mandatory gates exclude weighted scoring.
- [x] No candidate remains eligible for a weighted score; every gate result
  links to committed per-host evidence.
- [x] ADR-0005 exception scope, isolation, Rust share, packaging, replacement
  trigger, and removal criteria are explicit.
- [x] E4-009 remains blocked and names E4-008-05 as the required remediation.
