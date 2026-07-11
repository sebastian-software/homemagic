---
id: E1-007
epic: EPIC-001
title: Add bounded scheduling, recovery, and shutdown
status: done
priority: high
depends_on: [E1-004, E1-006]
adrs: [ADR-0006, ADR-0011]
created: 2026-07-11
updated: 2026-07-11
---

# E1-007: Runtime Resilience

## Outcome

Discovery, refresh, and sessions run continuously with bounded resource use,
explicit availability, deterministic recovery, and graceful shutdown.

## Tasks

- [x] Add startup and configurable periodic discovery schedules.
- [x] Deduplicate Shelly-specific and generic HTTP advertisements.
- [x] Bound DNS resolution, refresh concurrency, and global deadlines.
- [x] Add per-device timeouts so one device cannot block convergence.
- [x] Implement jittered exponential reconnect backoff with an upper bound.
- [x] Run bounded HTTP refresh after notification gaps.
- [x] Model sleeping devices without false failure transitions.
- [x] Coalesce duplicate observations before persistence and fan-out.
- [x] Record refresh summaries and per-device failure reasons.
- [x] Drain or cancel workers and sessions on shutdown.

## Acceptance criteria

- [x] One slow or unavailable device cannot delay other device updates.
- [x] Backoff stays within configured bounds and resets after recovery.
- [x] Freshness transitions to stale/offline without changing observed values.
- [x] Sleeping devices remain semantically distinct from failed devices.
- [x] Shutdown leaves no managed worker or write transaction running.

## Verification

- [x] Deterministic-clock backoff and freshness tests.
- [x] Network-loss, recovery, slow-device, and concurrency-bound tests.
- [x] Graceful-shutdown integration test.

## Progress log

- 2026-07-11: Issue created.
- 2026-07-11: Accepted ADR-0011 for bounded discovery, reconnect, gap-refresh,
  freshness, and shutdown loops.
- 2026-07-11: Added immediate and periodic reconciliation with global deadlines,
  a bounded subscription-gap channel, freshness evaluation, and producer-first
  shutdown.
- 2026-07-11: Added per-device refresh concurrency, HTTP timeouts, deterministic
  reconnect bounds and reset rules, session recovery, and slow-device fixtures.
- 2026-07-11: Verified with workspace format, Clippy, unit and integration tests,
  and documentation tests.
