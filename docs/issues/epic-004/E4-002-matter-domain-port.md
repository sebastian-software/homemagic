---
id: E4-002
epic: EPIC-004
title: Define the SDK-neutral Matter domain and controller port
status: done
priority: critical
depends_on: [E4-001]
adrs: [ADR-0001, ADR-0002, ADR-0033, ADR-0034]
created: 2026-07-12
updated: 2026-07-12
---

# E4-002: Matter Domain and Controller Port

## Outcome

Domain and application crates own a complete Matter controller contract that can
be implemented by the deterministic simulator and future SDK adapters without
leaking SDK types or raw protocol access to callers.

## Tasks

- [x] Add stable typed fabric, node, Matter endpoint, projection, subscription,
  controller-operation, and controller-event identities.
- [x] Define fabric/node/endpoint descriptors and immutable protocol metadata
  with bounded collections and versioned serialization.
- [x] Define desired state, reported state, freshness, convergence, uncertainty,
  report version, and projection revision contracts.
- [x] Define durable commissioning, cancellation, removal, export, restore, and
  repair operation phases with validated transitions.
- [x] Define redacted controller errors that preserve stable category, code,
  retryability, affected resource, and repair guidance.
- [x] Add the async `MatterController` port for fabric, commissioning,
  inventory, subscription, read, invoke, removal, export, and restore behavior.
- [x] Keep protocol invocation types adapter-private and impossible to construct
  through public RPC/MCP request models.
- [x] Add reusable controller-contract test inputs and observable outputs without
  coupling them to a concrete implementation.
- [x] Document crate ownership and permitted dependency direction.

## Acceptance criteria

- [x] No SDK dependency or SDK-owned type exists in domain/application contracts.
- [x] IDs survive mutable labels, addresses, sessions, and restarts.
- [x] All untrusted strings and collections have explicit bounds.
- [x] Secret-bearing values are non-serializable opaque references or sensitive
  inputs with redacted debug output.
- [x] The same port can represent simulator and production lifecycle behavior.

## Verification

- [x] Serialization and persisted-contract round trips pass.
- [x] Invalid operation transitions and malformed bounded values are rejected.
- [x] Error formatting and debug output pass secret-canary tests.
- [x] Port construction, mock implementation, and object-safety tests pass.
- [x] Dependency inspection proves no Matter SDK outside the integration crate.

## Evidence

- `homemagic-domain::matter` owns validated descriptors, bounded scalar values,
  desired/reported projections, operation state machines, normalized events, and
  closed redacted controller errors.
- `homemagic-application::MatterController` is an object-safe `Send + Sync`
  async port with closed command enums and non-serializable sensitive inputs.
- `crates/homemagic-domain/tests/persisted_contracts.rs` round-trips Matter
  descriptors, state, operations, events, and public errors.
- [Matter Controller Boundary](../../architecture/matter-controller-boundary.md)
  documents ownership and prohibited dependencies.
- `./scripts/check-matter-boundaries.sh` passed and remains the executable
  dependency guard for future simulator and production adapters.
- Workspace format, strict Clippy, all tests/features, Rustdoc with warnings
  denied, secret scan, and patch hygiene passed on 2026-07-12.

## Progress log

- 2026-07-12: Planned behind E4-001 decisions.
- 2026-07-12: Completed SDK-neutral domain and application contracts with
  validated deserialization, durable state machines, redacted sensitive values,
  an object-safe mockable port, persisted-contract tests, architecture guidance,
  and an executable dependency guard. E4-003 and E4-004 are ready.
