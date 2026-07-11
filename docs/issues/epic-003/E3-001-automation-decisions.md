---
id: E3-001
epic: EPIC-003
title: Accept automation engine decisions
status: done
priority: critical
depends_on: [EPIC-002]
adrs: [ADR-0004, ADR-0017, ADR-0018, ADR-0019, ADR-0020]
created: 2026-07-11
updated: 2026-07-11
---

# E3-001: Automation Decisions

## Tasks

- [x] Accept IR compatibility and normalized-plan versioning rules.
- [x] Accept deterministic scheduling, DST, missed-occurrence, and restart rules.
- [x] Accept capability Safety Profiles and simple approval/activation rules.
- [x] Accept automation version, run, trace, evidence, and draft retention rules.
- [x] Index the ADRs and cross-link the approved design and epic.

## Acceptance criteria

- [x] The decisions preserve ADR-0004's declarative, no-code boundary.
- [x] Missed schedule occurrences can never execute implicitly after restart.
- [x] Roller shutters are not treated the same as locks or critical valves.
- [x] Simulation/runtime parity and independent retention have explicit owners.

## Progress log

- 2026-07-11: Accepted ADR-0017 through ADR-0020 for independent document/plan
  compatibility, deterministic time and skipped misses, capability Safety
  Profiles, and protected independent automation retention.
