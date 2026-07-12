---
id: E4-001
epic: EPIC-004
title: Accept Matter controller boundaries and evaluation rules
status: ready
priority: critical
depends_on: [EPIC-001, EPIC-002]
adrs: [ADR-0002, ADR-0005, ADR-0008, ADR-0014, ADR-0015, ADR-0033, ADR-0034, ADR-0035, ADR-0036, ADR-0037, ADR-0038]
created: 2026-07-12
updated: 2026-07-12
---

# E4-001: Matter Decisions

## Outcome

HomeMagic has accepted, internally consistent decision records for the
controller boundary and every cross-cutting rule required before domain or
adapter implementation begins.

## Tasks

- [ ] Accept ADR-0033 for an SDK-neutral `MatterController` port and independent
  deterministic simulator.
- [ ] Accept ADR-0034 for descriptor/cluster-to-capability projection,
  invalidation, and namespaced diagnostic extensions.
- [ ] Accept ADR-0035 for short-lived, single-use interactive unlock
  authorization bound to exact actor, target, action, and desired revision.
- [ ] Accept ADR-0036 for shared pre-dispatch desired-state supersession and
  honest post-dispatch convergence.
- [ ] Accept ADR-0037 for HomeMagic fabric ownership, `SecretStore` references,
  encrypted export/restore, and incomplete-cleanup repair state.
- [ ] Accept ADR-0038 for the initial Matter-over-Wi-Fi boundary, explicit
  BLE/Thread limits, evidence classes, and fixed candidate scorecard.
- [ ] Define candidate scoring weights, mandatory gates, rejection rules, and
  tie-breaking before naming or testing candidates.
- [ ] Index and cross-link every ADR from the design, epic, and issue set.
- [ ] Verify the decisions do not alter ADR-0008's live-secret backend policy or
  expose raw protocol operations publicly.

## Acceptance criteria

- [ ] Application and domain code can be designed without choosing an SDK.
- [ ] Simulator success cannot satisfy protocol, hardware, or certification
  criteria.
- [ ] Unlock cannot be enabled by a broad space grant or automation approval.
- [ ] Every superseded command remains durably auditable.
- [ ] Headless Linux has no desktop-session or automatic plaintext dependency.
- [ ] Native Rust remains the default and every exception has ADR-0005 gates.

## Verification

- [ ] ADR status and index audit passes.
- [ ] Design, epic, plan, issue, and ADR links resolve.
- [ ] Placeholder scan contains no unresolved decision in E4-001 scope.
- [ ] Candidate scorecard is fixed before any candidate result is recorded.

## Progress log

- 2026-07-12: Issue created from the approved simulation-first design.
