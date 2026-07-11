---
id: E3-005
epic: EPIC-003
title: Simulate plans deterministically with virtual time
status: planned
priority: high
depends_on: [E3-003]
adrs: [ADR-0018]
created: 2026-07-11
updated: 2026-07-11
---

# E3-005: Deterministic Simulator

## Tasks

- [ ] Define scheduler, immutable-state, and command-evaluation ports.
- [ ] Implement virtual clock and deterministic ready-work ordering.
- [ ] Accept synthetic initial state, events, occurrences, and command outcomes.
- [ ] Interpret every trigger, condition, action, run mode, and failure policy.
- [ ] Model IANA timezone schedules and explicit DST semantics.
- [ ] Record missed schedules as skipped and support explicit catch-up simulation.
- [ ] Emit normalized ordered trace steps and reduced command intents.
- [ ] Add stable snapshots and repeated byte-equivalence tests.
- [ ] Prove the simulator cannot construct a physical dispatcher.

## Acceptance criteria

- [ ] Identical inputs produce byte-equivalent normalized traces.
- [ ] Simulation covers delays, waits, retries, timeouts, parallel and race groups.
- [ ] No simulation path can perform physical I/O.
- [ ] Simulation and compiler budgets terminate every fixture.
