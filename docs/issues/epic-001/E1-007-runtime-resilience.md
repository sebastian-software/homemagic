---
id: E1-007
epic: EPIC-001
title: Add bounded scheduling, recovery, and shutdown
status: planned
priority: high
depends_on: [E1-004, E1-006]
adrs: [ADR-0006]
created: 2026-07-11
updated: 2026-07-11
---

# E1-007: Runtime Resilience

## Outcome

Discovery, refresh, and sessions run continuously with bounded resource use,
explicit availability, deterministic recovery, and graceful shutdown.

## Tasks

- [ ] Add startup and configurable periodic discovery schedules.
- [ ] Deduplicate Shelly-specific and generic HTTP advertisements.
- [ ] Bound DNS resolution, refresh concurrency, and global deadlines.
- [ ] Add per-device timeouts so one device cannot block convergence.
- [ ] Implement jittered exponential reconnect backoff with an upper bound.
- [ ] Run bounded HTTP refresh after notification gaps.
- [ ] Model sleeping devices without false failure transitions.
- [ ] Coalesce duplicate observations before persistence and fan-out.
- [ ] Record refresh summaries and per-device failure reasons.
- [ ] Drain or cancel workers and sessions on shutdown.

## Acceptance criteria

- [ ] One slow or unavailable device cannot delay other device updates.
- [ ] Backoff stays within configured bounds and resets after recovery.
- [ ] Freshness transitions to stale/offline without changing observed values.
- [ ] Sleeping devices remain semantically distinct from failed devices.
- [ ] Shutdown leaves no managed worker or write transaction running.

## Verification

- [ ] Deterministic-clock backoff and freshness tests.
- [ ] Network-loss, recovery, slow-device, and concurrency-bound tests.
- [ ] Graceful-shutdown integration test.

## Progress log

- 2026-07-11: Issue created.
