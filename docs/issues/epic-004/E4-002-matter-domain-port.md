---
id: E4-002
epic: EPIC-004
title: Define the SDK-neutral Matter domain and controller port
status: planned
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

- [ ] Add stable typed fabric, node, Matter endpoint, projection, subscription,
  controller-operation, and controller-event identities.
- [ ] Define fabric/node/endpoint descriptors and immutable protocol metadata
  with bounded collections and versioned serialization.
- [ ] Define desired state, reported state, freshness, convergence, uncertainty,
  report version, and projection revision contracts.
- [ ] Define durable commissioning, cancellation, removal, export, restore, and
  repair operation phases with validated transitions.
- [ ] Define redacted controller errors that preserve stable category, code,
  retryability, affected resource, and repair guidance.
- [ ] Add the async `MatterController` port for fabric, commissioning,
  inventory, subscription, read, invoke, removal, export, and restore behavior.
- [ ] Keep protocol invocation types adapter-private and impossible to construct
  through public RPC/MCP request models.
- [ ] Add reusable controller-contract test inputs and observable outputs without
  coupling them to a concrete implementation.
- [ ] Document crate ownership and permitted dependency direction.

## Acceptance criteria

- [ ] No SDK dependency or SDK-owned type exists in domain/application contracts.
- [ ] IDs survive mutable labels, addresses, sessions, and restarts.
- [ ] All untrusted strings and collections have explicit bounds.
- [ ] Secret-bearing values are non-serializable opaque references or sensitive
  inputs with redacted debug output.
- [ ] The same port can represent simulator and production lifecycle behavior.

## Verification

- [ ] Serialization and persisted-contract round trips pass.
- [ ] Invalid operation transitions and malformed bounded values are rejected.
- [ ] Error formatting and debug output pass secret-canary tests.
- [ ] Port construction, mock implementation, and object-safety tests pass.
- [ ] Dependency inspection proves no Matter SDK outside the integration crate.

## Progress log

- 2026-07-12: Planned behind E4-001 decisions.
