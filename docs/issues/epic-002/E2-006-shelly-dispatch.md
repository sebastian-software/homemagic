---
id: E2-006
epic: EPIC-002
title: Dispatch common commands to Shelly safely
status: in_progress
priority: high
depends_on: [E2-005]
adrs: [ADR-0006, ADR-0014, ADR-0015]
created: 2026-07-11
updated: 2026-07-11
---

# E2-006: Shelly Command Dispatch

## Tasks

- [x] Map `on_off.v1` set/toggle to typed Shelly component calls.
- [x] Map `level.v1` with validated range and transition duration.
- [x] Map `position.v1` open/close/stop/go-to-position.
- [x] Reject uncalibrated or unsupported position requests before RPC.
- [x] Normalize acknowledgement, protection, obstruction, thermal, and RPC errors.
- [ ] Confirm from push observations with bounded read fallback.
- [ ] Add fixtures for success, timeout, reconnect, mismatch, and duplicate prevention.

## Acceptance criteria

- [x] No public raw Shelly method or JSON payload bypass exists.

## Progress log

- 2026-07-11: Added private typed mappings for `Switch.Set`/`Toggle`,
  `Light.Set`/`Toggle`, and `Cover.Open`/`Close`/`Stop`/`GoToPosition`, including
  component IDs, bounded transitions, and the `homemagic` origin tag.
- 2026-07-11: Added stable normalization for calibration/precondition, thermal,
  obstruction, electrical/safety protection, and generic RPC rejection errors.
  The mapping follows Shelly's official Gen2 Switch, Light, and Cover contracts.
- 2026-07-11: All 29 Shelly tests pass; transport, reconnect, and push/read
  confirmation remain in this issue.
- [ ] Retry/reconnect cannot produce a second physical dispatch.
- [ ] Confirmation reports observed state separately from acknowledgement.
