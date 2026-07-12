---
id: E4-008
epic: EPIC-004
title: Evaluate and select a production Matter controller implementation
status: in_progress
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
| [E4-008-02](E4-008-02-native-build-audit.md) | In progress | Cross-platform native build and footprint evidence |
| [E4-008-03](E4-008-03-contract-reference.md) | Planned | Fixed port contract and independent reference lifecycle |
| [E4-008-04](E4-008-04-selection-adr.md) | Planned | Evidence matrix and accepted ADR-0039 |

## Outcome

A current, reproducible, evidence-backed candidate study selects or rejects a
production controller approach without weakening the fixed contract, platform,
security, packaging, or Rust-majority requirements.

## Tasks

- [ ] Re-discover credible controller candidates from current primary sources;
  do not rely on the planning-date ecosystem snapshot.
- [ ] Record exact source revision, release, license, provenance, maintenance,
  disclosed conformance/certification status, and security posture.
- [ ] Pin reproducible candidate spikes outside production dependency graphs.
- [ ] Build every credible Rust-native candidate on macOS ARM64 and Linux x86_64.
- [ ] Run all supported `MatterController` contract cases and record unsupported
  cases explicitly.
- [ ] Exercise fabric persistence hooks, commissioning, inventory, read, invoke,
  subscriptions, restart, and removal against independent fixtures/reference
  tools where possible.
- [ ] Measure first-party Rust share, unsafe blocks, transitive native code, FFI,
  binary/runtime dependencies, binary size, and packaging complexity.
- [ ] Score failure isolation, diagnostics, replacement cost, and ability to keep
  SDK types inside `homemagic-matter`.
- [ ] Record rejected candidates and exact mandatory-gate failures.
- [ ] Accept ADR-0039 selecting native Rust, narrow FFI, or isolated sidecar only
  from the committed matrix and evidence.
- [ ] Define replacement triggers and removal criteria for every exception.

## Acceptance criteria

- [ ] The scorecard predates results and is applied consistently.
- [ ] Every claim links to source, command, fixture, host, and captured output.
- [ ] A candidate cannot pass solely against its own simulated implementation.
- [ ] Any non-Rust exception meets every ADR-0005 requirement and preserves a
  narrow replaceable boundary.
- [ ] If no candidate passes, the issue records a scoped blocker instead of
  silently reducing product requirements.

## Verification

- [ ] Candidate build and test scripts reproduce from a clean checkout.
- [ ] macOS ARM64 and Linux x86_64 reports are separate and complete.
- [ ] License/provenance, unsafe/FFI, Rust-share, and packaging audits pass or
  have explicit rejection evidence.
- [ ] ADR-0039 maps every selected trade-off to the fixed scorecard.
- [ ] Production manifests do not include rejected/reference-only dependencies.

## Progress log

- 2026-07-12: Planning deliberately names no winner; ecosystem evidence must be
  refreshed when this issue starts.
- 2026-07-12: Current primary-source discovery found one credible native Rust
  controller candidate, `rust-matc`; `rs-matter` is a device/server reference,
  not a controller. The detailed rubric was frozen and work decomposed into four
  dependency-ordered children before assigning scores.
- 2026-07-12: E4-008-01 pin verification passed public CI run `29209739369`;
  E4-008-02 is ready.
