---
id: E4-004
epic: EPIC-004
title: Implement the deterministic Rust Matter controller simulator
status: done
priority: critical
depends_on: [E4-002]
adrs: [ADR-0001, ADR-0033, ADR-0034, ADR-0038]
created: 2026-07-12
updated: 2026-07-12
---

# E4-004: Deterministic Controller Simulator

## Outcome

An in-process Rust implementation of `MatterController` provides deterministic
light and lock behavior, lifecycle operations, restart checkpoints, and fault
injection for application development without claiming protocol compatibility.

## Tasks

- [x] Add the `homemagic-matter` workspace crate without a production SDK.
- [x] Implement virtual clock, deterministic identity, fabric, node, endpoint,
  attribute, event, command, and subscription state.
- [x] Add a versioned On/Off light fixture.
- [x] Add a versioned Door Lock fixture with reported state and lock/unlock
  command behavior.
- [x] Add dispatch barriers before invocation and after acknowledgement.
- [x] Script delayed, duplicate, dropped, and out-of-order reports.
- [x] Script subscription loss, reconnect, resubscription, and bounded-read
  outcomes.
- [x] Script restart during commissioning, cancellation, removal, export,
  restore, projection, and subscription phases.
- [x] Use deterministic non-secret placeholders that cannot be imported by a
  production adapter.
- [x] Run the reusable controller-contract suite against the simulator.
- [x] Commit normalized fixtures and byte-stable expected event traces.

## Acceptance criteria

- [x] Identical input produces byte-equivalent normalized output across runs.
- [x] The simulator exercises every controller port operation and error class.
- [x] Tests can deterministically distinguish supersession before dispatch from
  convergence after dispatch.
- [x] Simulator exports are structurally rejected by production import paths.
- [x] Documentation labels the simulator as application-contract evidence only.

## Verification

- [x] Light and lock happy-path contract suites pass.
- [x] Every injected fault has a stable expected trace and terminal/recovery state.
- [x] Repeated-run and randomized-order property tests are deterministic.
- [x] macOS ARM64 and Linux x86_64 produce identical normalized fixture hashes.
- [x] Dependency inspection shows no Matter SDK or external reference runtime.

## Evidence

- `homemagic-matter::DeterministicMatterSimulator` implements every
  `MatterController` method with virtual time and SDK-neutral state.
- [Deterministic Matter Controller Simulator](../../architecture/matter-simulator.md)
  documents the evidence boundary, fixtures, barriers, faults, checkpoints,
  export isolation, committed trace, and platform status.
- `controller_contract.rs` passes nine scenarios including a Proptest-generated
  repeated-order property and all twelve scripted nonterminal restart phases.
- `light-trace-v1.json` and `light-trace-v1.sha256` fix the normalized trace to
  `7451b5a74337e40a2312f5a5723308ad1e8a881714e19f94c9b0e538bff1f244`.
- [CI run 29196515664](https://github.com/sebastian-software/homemagic/actions/runs/29196515664)
  passed the same committed hash on Linux x86_64 and macOS ARM64 with explicit
  `uname -m` assertions. Jobs `86660357969` and `86660357907` prove the two
  platform results.
- Workspace format, strict Clippy, all tests/features, warning-denied Rustdoc,
  dependency boundaries, secret scan, and patch hygiene passed on 2026-07-12.

## Progress log

- 2026-07-12: Planned as the first executable Matter implementation.
- 2026-07-12: Implemented the pure-Rust simulator, versioned light and lock,
  complete port behavior, barriers, ordered faults, restart checkpoints,
  typed export isolation, property tests, and committed normalized trace.
  The first public CI run then confirmed the exact committed trace hash on both
  Linux x86_64 and macOS ARM64, completing E4-004 and making E4-005 ready.
