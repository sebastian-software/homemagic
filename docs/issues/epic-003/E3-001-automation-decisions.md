---
id: E3-001
epic: EPIC-003
title: Accept automation engine decisions
status: ready
priority: critical
depends_on: [EPIC-002]
adrs: [ADR-0004, ADR-0017, ADR-0018, ADR-0019, ADR-0020]
created: 2026-07-11
updated: 2026-07-11
---

# E3-001: Automation Decisions

## Tasks

- [ ] Accept IR compatibility and normalized-plan versioning rules.
- [ ] Accept deterministic scheduling, DST, missed-occurrence, and restart rules.
- [ ] Accept capability Safety Profiles and simple approval/activation rules.
- [ ] Accept automation version, run, trace, evidence, and draft retention rules.
- [ ] Index the ADRs and cross-link the approved design and epic.

## Acceptance criteria

- [ ] The decisions preserve ADR-0004's declarative, no-code boundary.
- [ ] Missed schedule occurrences can never execute implicitly after restart.
- [ ] Roller shutters are not treated the same as locks or critical valves.
- [ ] Simulation/runtime parity and independent retention have explicit owners.
