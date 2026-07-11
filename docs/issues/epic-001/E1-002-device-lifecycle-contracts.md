---
id: E1-002
epic: EPIC-001
title: Define durable device lifecycle and event contracts
status: ready
priority: critical
depends_on: [E1-001]
adrs: [ADR-0002]
created: 2026-07-11
updated: 2026-07-11
---

# E1-002: Device Lifecycle Contracts

## Outcome

Infrastructure-independent types express durable identity, enrollment,
availability, freshness, observations, diagnostics, repair needs, and causal
events without conflating desired and observed state.

## Tasks

- [ ] Add integration-instance and installation identity types.
- [ ] Model candidate, enrolled, stale, removed, and rediscovered transitions.
- [ ] Model online, degraded, offline, sleeping, and unknown availability.
- [ ] Add first-seen, last-seen, last-success, observed-at, and stale-at data.
- [ ] Version capability descriptors independently from display metadata.
- [ ] Add typed observation and lifecycle events with causation metadata.
- [ ] Add identity-collision and credential-repair records.
- [ ] Define repository, event-sink, clock, and session application ports.

## Acceptance criteria

- [ ] Invalid lifecycle transitions are rejected by domain code.
- [ ] Freshness is deterministic under an injected clock.
- [ ] Stable IDs are unaffected by names, spaces, aliases, or network addresses.
- [ ] Partial observations preserve unchanged values and source timestamps.
- [ ] Public errors and diagnostics are serializable and secret-safe.

## Verification

- [ ] Unit tests cover every lifecycle transition.
- [ ] Unit tests cover freshness boundaries and sleeping-device behavior.
- [ ] Serialization round trips cover all persisted domain types.
- [ ] Public API documentation includes examples for core types.

## Progress log

- 2026-07-11: Issue created.
