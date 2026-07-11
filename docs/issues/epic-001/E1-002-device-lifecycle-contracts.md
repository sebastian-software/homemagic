---
id: E1-002
epic: EPIC-001
title: Define durable device lifecycle and event contracts
status: done
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

- [x] Add integration-instance and installation identity types. Evidence:
  `crates/homemagic-domain/src/identity.rs`.
- [x] Model candidate, enrolled, stale, removed, and rediscovered transitions.
  Evidence: `DeviceLifecycle`, `LifecycleTrigger`, and the exhaustive transition
  test in `crates/homemagic-domain/src/lifecycle.rs`.
- [x] Model online, degraded, offline, sleeping, and unknown availability.
  Evidence: `AvailabilityState` and `Availability`.
- [x] Add first-seen, last-seen, last-success, observed-at, and freshness data.
  Evidence: `DeviceTimestamps`, `ObservedValue`, and `FreshnessPolicy`.
- [x] Version capability descriptors independently from display metadata.
  Evidence: `CapabilityDescriptor` and the identity-stability test.
- [x] Add typed observation and lifecycle events with causation metadata.
  Evidence: `CapabilityObservation`, `DomainEvent`, and `CausationMetadata`.
- [x] Add identity-collision and credential-repair records. Evidence:
  `RepairKind` and `RepairRecord`.
- [x] Define repository, event-sink, clock, and session application ports.
  Evidence: `crates/homemagic-application/src/ports.rs`.

## Acceptance criteria

- [x] Invalid lifecycle transitions are rejected by domain code. Evidence:
  exhaustive state/trigger matrix test.
- [x] Freshness is deterministic under an injected clock. Evidence: explicit-time
  freshness tests and the application `Clock` port test.
- [x] Stable IDs are unaffected by names, spaces, aliases, or network addresses.
  Evidence: integration-instance and mutable-metadata identity tests.
- [x] Partial observations preserve unchanged values and source timestamps.
  Evidence: `partial_merge_should_preserve_unchanged_fields` and
  `partial_merge_should_ignore_older_field_value`.
- [x] Public errors and diagnostics are serializable and secret-safe. Evidence:
  `public_errors_should_serialize_without_runtime_context` and structured repair
  payloads without protocol secret fields.

## Verification

- [x] Unit tests cover every lifecycle transition. Evidence:
  `lifecycle_should_cover_every_state_and_trigger_pair`.
- [x] Unit tests cover freshness boundaries and sleeping-device behavior.
  Evidence: four freshness tests in `lifecycle.rs`.
- [x] Serialization round trips cover all persisted domain types. Evidence:
  `crates/homemagic-domain/tests/persisted_contracts.rs`.
- [x] Public API documentation includes examples for core types. Evidence: two
  passing `homemagic-domain` doctests.

## Progress log

- 2026-07-11: Issue created.
- 2026-07-11: Implementation started across focused domain modules and
  application ports.
- 2026-07-11: Completed. Evidence: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
  and `cargo test --workspace --all-features --locked`.
