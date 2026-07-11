---
id: E1-006
epic: EPIC-001
title: Maintain Shelly WebSocket observation sessions
status: ready
priority: high
depends_on: [E1-004, E1-005]
adrs: [ADR-0006, ADR-0010]
created: 2026-07-11
updated: 2026-07-11
---

# E1-006: Shelly Managed Sessions

## Outcome

Each active Shelly device has at most one managed WebSocket RPC session that
normalizes status and event notifications into durable observations and typed
events.

## Tasks

- [x] Add a per-device session supervisor owned by the Shelly adapter. Evidence:
  `ShellySessionSupervisor` implements the application session lifecycle port
  with atomic replace semantics keyed by `DeviceId`.
- [ ] Authenticate WebSocket RPC without exposing credentials.
- [x] Parse `NotifyStatus` and `NotifyEvent` frames. Evidence: strict
  `parse_notification` support for status, full-status, and event envelopes.
- [x] Merge partial component updates into current observations. Evidence:
  `StatusCache::apply` recursively overlays component fields and removes
  explicit `null` values.
- [x] Preserve unchanged values and their observation timestamps. Evidence:
  omitted component fields remain in the complete session baseline; domain
  observation merging already retains field timestamps independently.
- [x] Deduplicate replayed or identical notifications. Evidence: identical
  status patches yield no changed components and `EventDeduplicator` maintains
  a bounded replay window.
- [ ] Detect sequence or subscription gaps and request refresh fallback.
- [ ] Stop replaced, removed, and shutdown sessions cleanly.

## Acceptance criteria

- [ ] A physical-status fixture updates state without explicit refresh.
- [x] Partial frames do not erase unchanged component fields. Evidence:
  `partial_status_should_preserve_unchanged_fields_and_remove_nulls`.
- [ ] Duplicate frames do not create duplicate persisted events.
- [x] No device has more than one active managed session. Evidence:
  `replacement_should_never_overlap_same_device` observes a maximum of one
  active runner across replacement.
- [ ] Malformed frames degrade one session without crashing the runtime.

## Verification

- [x] Recorded full-status, partial-status, event, and malformed-frame tests.
  Evidence: sanitized `notify_*.json` fixtures and parser/cache tests.
- [x] Session uniqueness and cancellation tests. Evidence: replacement, stop,
  and multi-device shutdown tests join every owned task.
- [x] Observation merge and deduplication tests. Evidence: cache idempotency,
  partial overlay, older-frame gap, and event replay tests.

## Progress log

- 2026-07-11: Issue created.
- 2026-07-11: Accepted ADR-0010 for per-device ownership, baseline overlays,
  idempotency, cancellation, and gap-triggered refresh semantics.
- 2026-07-11: Implemented and verified notification parsing, full/partial cache
  semantics, timestamp-regression gap detection, and bounded event replay
  filtering. Session supervision and WebSocket transport remain in progress.
- 2026-07-11: Added the adapter-owned session supervisor with replace, stop,
  shutdown, task joining, and deterministic uniqueness/cancellation tests.
  WebSocket transport and runtime lifecycle wiring remain in progress.
