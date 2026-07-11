---
id: E2-006
epic: EPIC-002
title: Dispatch common commands to Shelly safely
status: planned
priority: high
depends_on: [E2-005]
adrs: [ADR-0006, ADR-0014, ADR-0015]
created: 2026-07-11
updated: 2026-07-11
---

# E2-006: Shelly Command Dispatch

## Tasks

- [ ] Map `on_off.v1` set/toggle to typed Shelly component calls.
- [ ] Map `level.v1` with validated range and transition duration.
- [ ] Map `position.v1` open/close/stop/go-to-position.
- [ ] Reject uncalibrated or unsupported position requests before RPC.
- [ ] Normalize acknowledgement, protection, obstruction, thermal, and RPC errors.
- [ ] Confirm from push observations with bounded read fallback.
- [ ] Add fixtures for success, timeout, reconnect, mismatch, and duplicate prevention.

## Acceptance criteria

- [ ] No public raw Shelly method or JSON payload bypass exists.
- [ ] Retry/reconnect cannot produce a second physical dispatch.
- [ ] Confirmation reports observed state separately from acknowledgement.
