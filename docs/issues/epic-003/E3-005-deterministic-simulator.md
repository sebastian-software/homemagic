---
id: E3-005
epic: EPIC-003
title: Simulate plans deterministically with virtual time
status: done
priority: high
depends_on: [E3-003]
adrs: [ADR-0018]
created: 2026-07-11
updated: 2026-07-11
---

# E3-005: Deterministic Simulator

## Tasks

- [x] Define scheduler, immutable-state, and command-evaluation ports.
- [x] Implement virtual clock and deterministic ready-work ordering.
- [x] Accept synthetic initial state, events, occurrences, and command outcomes.
- [x] Interpret every trigger, condition, action, run mode, and failure policy.
- [x] Model IANA timezone schedules and explicit DST semantics.
- [x] Record missed schedules as skipped and support explicit catch-up simulation.
- [x] Emit normalized ordered trace steps and reduced command intents.
- [x] Add stable snapshots and repeated byte-equivalence tests.
- [x] Prove the simulator cannot construct a physical dispatcher.

## Acceptance criteria

- [x] Identical inputs produce byte-equivalent normalized traces.
- [x] Simulation covers delays, waits, retries, timeouts, parallel and race groups.
- [x] No simulation path can perform physical I/O.
- [x] Simulation and compiler budgets terminate every fixture.

## Evidence

- Data-only simulator construction plus separate scheduler, immutable-state, and
  command-evaluation port contracts.
- Typed fixtures cover every trigger family, every run mode, self-trigger rules,
  waits/timeouts, retries/backoff, failure policies/fallbacks, branches,
  parallel/race groups, and compiler-owned budgets.
- IANA timezone enumeration has an explicit Europe/Berlin spring DST test;
  expired occurrences are `missed_skipped` unless catch-up is explicit.
- Repeated complete trace serialization is byte-equal and the committed
  normalized snapshot fixes trace order, virtual times, outcomes, variables,
  and trace hash.
- Detailed contract: [Deterministic Automation Simulation](../../architecture/automation-simulation.md).
