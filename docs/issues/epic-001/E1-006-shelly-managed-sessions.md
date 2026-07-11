---
id: E1-006
epic: EPIC-001
title: Maintain Shelly WebSocket observation sessions
status: planned
priority: high
depends_on: [E1-004, E1-005]
adrs: [ADR-0006]
created: 2026-07-11
updated: 2026-07-11
---

# E1-006: Shelly Managed Sessions

## Outcome

Each active Shelly device has at most one managed WebSocket RPC session that
normalizes status and event notifications into durable observations and typed
events.

## Tasks

- [ ] Add a per-device session supervisor owned by the Shelly adapter.
- [ ] Authenticate WebSocket RPC without exposing credentials.
- [ ] Parse `NotifyStatus` and `NotifyEvent` frames.
- [ ] Merge partial component updates into current observations.
- [ ] Preserve unchanged values and their observation timestamps.
- [ ] Deduplicate replayed or identical notifications.
- [ ] Detect sequence or subscription gaps and request refresh fallback.
- [ ] Stop replaced, removed, and shutdown sessions cleanly.

## Acceptance criteria

- [ ] A physical-status fixture updates state without explicit refresh.
- [ ] Partial frames do not erase unchanged component fields.
- [ ] Duplicate frames do not create duplicate persisted events.
- [ ] No device has more than one active managed session.
- [ ] Malformed frames degrade one session without crashing the runtime.

## Verification

- [ ] Recorded full-status, partial-status, event, and malformed-frame tests.
- [ ] Session uniqueness and cancellation tests.
- [ ] Observation merge and deduplication tests.

## Progress log

- 2026-07-11: Issue created.
