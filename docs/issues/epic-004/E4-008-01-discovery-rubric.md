---
id: E4-008-01
epic: EPIC-004
parent: E4-008
title: Refresh controller candidates and freeze the detailed rubric
status: done
priority: critical
depends_on: [E4-004]
adrs: [ADR-0005, ADR-0033, ADR-0038]
created: 2026-07-12
updated: 2026-07-12
---

# E4-008-01: Candidate Discovery and Rubric

## Outcome

Current primary sources produce a screened candidate set, exact pins, and a
detailed immutable 0–5 rubric before candidate scores are assigned.

## Tasks

- [x] Search current controller repositories and releases.
- [x] Record exact revisions, roles, licenses, and initial mandatory screens.
- [x] Freeze the detailed weighted score thresholds.
- [x] Capture provenance, maintenance, security, and conformance source links.
- [x] Add a machine-readable candidate manifest and integrity checks.

## Verification

- [x] Discovery and manifest checks run from a clean checkout.
- [x] Public CI validates the committed candidate pins.

## Progress log

- 2026-07-12: Current primary-source discovery screened six sources, advanced
  `rust-matc` to the full native spike, retained `rs-matter` only as an
  independent device/reference candidate, and recorded mandatory rejections and
  ADR-0005 contingencies. The detailed rubric, machine-readable pins, and clean
  fetch validation are committed; public CI remains pending.
- 2026-07-12: Public CI run `29209739369` fetched and verified every exact pin,
  then passed the full Linux quality and cross-platform simulator gates. This
  issue is done; E4-008-02 is ready.
