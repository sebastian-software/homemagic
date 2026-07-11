---
id: E2-006
epic: EPIC-002
title: Dispatch common commands to Shelly safely
status: done
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
- [x] Confirm from push observations with bounded read fallback.
- [x] Add fixtures for success, timeout, reconnect, mismatch, and duplicate prevention.

## Acceptance criteria

- [x] No public raw Shelly method or JSON payload bypass exists.
- [x] Retry/reconnect cannot produce a second physical dispatch.
- [x] Confirmation reports observed state separately from acknowledgement.

## Progress log

- 2026-07-11: Added private typed mappings for `Switch.Set`/`Toggle`,
  `Light.Set`/`Toggle`, and `Cover.Open`/`Close`/`Stop`/`GoToPosition`, including
  component IDs, bounded transitions, and the `homemagic` origin tag.
- 2026-07-11: Added stable normalization for calibration/precondition, thermal,
  obstruction, electrical/safety protection, and generic RPC rejection errors.
  The mapping follows Shelly's official Gen2 Switch, Light, and Cover contracts.
- 2026-07-11: Added the typed HTTP command adapter with bounded Digest
  authentication, stable transport/RPC failures, and no public raw request path.
- 2026-07-11: Added push-first observed confirmation with one bounded status-read
  fallback. Success, timeout, Digest reconnect, mismatched observation, and
  duplicate-prevention fixtures pass across 33 Shelly tests.
- 2026-07-11: Materialized fresh `toggle` requests to explicit target state before
  durable dispatch so acknowledgement and confirmation share one concrete goal.
  Post-dispatch recovery remains confirmation-only and never blindly redispatches.
- 2026-07-11: Implemented against Shelly's official Gen2 component contracts:
  <https://shelly-api-docs.shelly.cloud/gen2/ComponentsAndServices/Light/> and
  <https://shelly-api-docs.shelly.cloud/gen2/ComponentsAndServices/Cover/>.
