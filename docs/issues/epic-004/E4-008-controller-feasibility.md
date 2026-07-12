---
id: E4-008
epic: EPIC-004
title: Evaluate and select a production Matter controller implementation
status: done
priority: critical
depends_on: [E4-004]
adrs: [ADR-0005, ADR-0033, ADR-0038, ADR-0039]
created: 2026-07-12
updated: 2026-07-12
---

# E4-008: Controller Feasibility and Selection

## Child issues

| Issue | Status | Outcome |
| --- | --- | --- |
| [E4-008-01](E4-008-01-discovery-rubric.md) | Done | Current candidate screen and frozen detailed rubric |
| [E4-008-02](E4-008-02-native-build-audit.md) | Done | Cross-platform native build and footprint evidence |
| [E4-008-03](E4-008-03-contract-reference.md) | Done | Fixed port contract and independent reference lifecycle |
| [E4-008-04](E4-008-04-selection-adr.md) | Done | No candidate selected; remediation required |

## Outcome

A current, reproducible, evidence-backed candidate study selects or rejects a
production controller approach without weakening the fixed contract, platform,
security, packaging, or Rust-majority requirements.

## Tasks

- [x] Re-discover credible controller candidates from current primary sources;
  do not rely on the planning-date ecosystem snapshot.
- [x] Record exact source revision, release, license, provenance, maintenance,
  disclosed conformance/certification status, and security posture.
- [x] Pin reproducible candidate spikes outside production dependency graphs.
- [x] Build every credible Rust-native candidate on macOS ARM64 and Linux x86_64.
- [x] Run all supported `MatterController` contract cases and record unsupported
  cases explicitly.
- [x] Exercise fabric persistence hooks, commissioning, inventory, read, invoke,
  subscriptions, restart, and removal against independent fixtures/reference
  tools where possible.
- [x] Measure first-party Rust share, unsafe blocks, transitive native code, FFI,
  binary/runtime dependencies, binary size, and packaging complexity.
- [x] Evaluate failure isolation, diagnostics, replacement cost, and ability to
  keep SDK types inside `homemagic-matter`; exclude failed candidates from
  weighted scoring.
- [x] Record rejected candidates and exact mandatory-gate failures.
- [x] Accept ADR-0039 from the committed matrix, selecting no candidate because
  none passes every gate.
- [x] Define replacement triggers and removal criteria for every proposed
  exception.

## Acceptance criteria

- [x] The scorecard predates results and is applied consistently.
- [x] Every selection claim links to source, command, fixture, host, and
  captured output.
- [x] A candidate cannot pass solely against its own simulated implementation.
- [x] No non-Rust exception is accepted without every ADR-0005 requirement and
  a narrow replaceable boundary.
- [x] Since no candidate passes, the issue records a scoped blocker instead of
  silently reducing product requirements.

## Verification

- [x] Candidate build and test scripts reproduce from a clean checkout.
- [x] macOS ARM64 and Linux x86_64 reports are separate and complete.
- [x] License/provenance, unsafe/FFI, Rust-share, and packaging audits pass or
  have explicit rejection evidence.
- [x] ADR-0039 maps the no-selection outcome to the fixed scorecard.
- [x] Production manifests do not include rejected/reference-only dependencies.

## Progress log

- 2026-07-12: Planning deliberately names no winner; ecosystem evidence must be
  refreshed when this issue starts.
- 2026-07-12: Current primary-source discovery found one credible native Rust
  controller candidate, `rust-matc`; `rs-matter` is a device/server reference,
  not a controller. The detailed rubric was frozen and work decomposed into four
  dependency-ordered children before assigning scores.
- 2026-07-12: E4-008-01 pin verification passed public CI run `29209739369`;
  E4-008-02 is ready.
- 2026-07-12: E4-008-02 public candidate run `29210089483` passed both hosts;
  exact reports and the footprint audit are committed pending final repository
  CI.
- 2026-07-12: E4-008-02 passed final repository CI run `29210237059`; E4-008-03
  is ready.
- 2026-07-12: Source-level review found that `rs-matter` at the already pinned
  revision includes a real commissioner/controller path omitted by the
  device-oriented README summary. E4-008-02 was reopened to audit that second
  credible native candidate before contract comparison begins.
- 2026-07-12: Independent public run `29211713067` failed the fixed lifecycle
  differently by host: Linux timed out during commissioning after
  `ArmFailSafe`; macOS reached invoke after successful commission/read/
  subscribe and failed there. E4-008-03 is complete evidence; neither native
  candidate can advance to weighted selection.
- 2026-07-12: Corrected two-candidate build/footprint run `29211681113` passed
  all four jobs; E4-008-02 is closed with exact unsafe and all-feature outcomes.
- 2026-07-12: Official SDK and matter.js contingencies also fail mandatory
  boundary or independent-lifecycle gates. ADR-0039 selects no production
  controller; E4-008-05 is the executable remediation blocker for E4-009.
