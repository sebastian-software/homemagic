---
id: E4-004
epic: EPIC-004
title: Implement the deterministic Rust Matter controller simulator
status: planned
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

- [ ] Add the `homemagic-matter` workspace crate without a production SDK.
- [ ] Implement virtual clock, deterministic identity, fabric, node, endpoint,
  attribute, event, command, and subscription state.
- [ ] Add a versioned On/Off light fixture.
- [ ] Add a versioned Door Lock fixture with reported state and lock/unlock
  command behavior.
- [ ] Add dispatch barriers before invocation and after acknowledgement.
- [ ] Script delayed, duplicate, dropped, and out-of-order reports.
- [ ] Script subscription loss, reconnect, resubscription, and bounded-read
  outcomes.
- [ ] Script restart during commissioning, cancellation, removal, export,
  restore, projection, and subscription phases.
- [ ] Use deterministic non-secret placeholders that cannot be imported by a
  production adapter.
- [ ] Run the reusable controller-contract suite against the simulator.
- [ ] Commit normalized fixtures and byte-stable expected event traces.

## Acceptance criteria

- [ ] Identical input produces byte-equivalent normalized output across runs.
- [ ] The simulator exercises every controller port operation and error class.
- [ ] Tests can deterministically distinguish supersession before dispatch from
  convergence after dispatch.
- [ ] Simulator exports are structurally rejected by production import paths.
- [ ] Documentation labels the simulator as application-contract evidence only.

## Verification

- [ ] Light and lock happy-path contract suites pass.
- [ ] Every injected fault has a stable expected trace and terminal/recovery state.
- [ ] Repeated-run and randomized-order property tests are deterministic.
- [ ] macOS ARM64 and Linux x86_64 produce identical normalized fixture hashes.
- [ ] Dependency inspection shows no Matter SDK or external reference runtime.

## Progress log

- 2026-07-12: Planned as the first executable Matter implementation.
