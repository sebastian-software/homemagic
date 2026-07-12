---
id: E4-001
epic: EPIC-004
title: Accept Matter controller boundaries and evaluation rules
status: done
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

- [x] Accept ADR-0033 for an SDK-neutral `MatterController` port and independent
  deterministic simulator.
- [x] Accept ADR-0034 for descriptor/cluster-to-capability projection,
  invalidation, and namespaced diagnostic extensions.
- [x] Accept ADR-0035 for short-lived, single-use interactive unlock
  authorization bound to exact actor, target, action, and desired revision.
- [x] Accept ADR-0036 for shared pre-dispatch desired-state supersession and
  honest post-dispatch convergence.
- [x] Accept ADR-0037 for HomeMagic fabric ownership, `SecretStore` references,
  encrypted export/restore, and incomplete-cleanup repair state.
- [x] Accept ADR-0038 for the initial Matter-over-Wi-Fi boundary, explicit
  BLE/Thread limits, evidence classes, and fixed candidate scorecard.
- [x] Define candidate scoring weights, mandatory gates, rejection rules, and
  tie-breaking before naming or testing candidates.
- [x] Index and cross-link every ADR from the design, epic, and issue set.
- [x] Verify the decisions do not alter ADR-0008's live-secret backend policy or
  expose raw protocol operations publicly.

## Acceptance criteria

- [x] Application and domain code can be designed without choosing an SDK.
- [x] Simulator success cannot satisfy protocol, hardware, or certification
  criteria.
- [x] Unlock cannot be enabled by a broad space grant or automation approval.
- [x] Every superseded command remains durably auditable.
- [x] Headless Linux has no desktop-session or automatic plaintext dependency.
- [x] Native Rust remains the default and every exception has ADR-0005 gates.

## Verification

- [x] ADR status and index audit passes.
- [x] Design, epic, plan, issue, and ADR links resolve.
- [x] Placeholder scan contains no unresolved decision in E4-001 scope.
- [x] Candidate scorecard is fixed before any candidate result is recorded.

## Progress log

- 2026-07-12: Issue created from the approved simulation-first design.
- 2026-07-12: Accepted and indexed ADR-0033 through ADR-0038. The fixed
  scorecard has eight mandatory gates, six weighted categories totaling 100,
  and deterministic tie-breaking. No production controller candidate is
  selected; E4-002 is ready.
