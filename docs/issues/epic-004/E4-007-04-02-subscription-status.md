---
id: E4-007-04-02
epic: EPIC-004
parent: E4-007-04
title: Derive durable subscription freshness and repair guidance
status: ready
priority: high
depends_on: [E4-007-04-01]
adrs: [ADR-0033, ADR-0034, ADR-0041]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-04-02: Subscription Status

## Outcome

Each logical subscription exposes deterministic freshness, retry budget, gap
state, and stable remediation guidance derived from durable facts at an explicit
evaluation time.

## Tasks

- [ ] Define subscription diagnostic status and freshness DTOs.
- [ ] Persist bounded recovery counters, retry deadline, and last gap-read time.
- [ ] Derive established, stale, waiting, exhausted, and repair-required status.
- [ ] Preserve sleepy-device read throttling in status calculations.
- [ ] Expose stable remediation codes without adapter text.

## Acceptance criteria

- [ ] Status evaluation has no wall-clock or I/O side effects.
- [ ] Retry and gap budgets cannot reset through reads or restart.
- [ ] Exhaustion remains visible until explicit repair succeeds.

## Verification

- [ ] Fresh, stale, sleepy, waiting, exhausted, and reopen matrices pass.
- [ ] Boundary timestamps and retry counters are deterministic.

## Progress log

- 2026-07-12: E4-007-04-01 completed with public cross-platform CI. This issue
  is ready.
