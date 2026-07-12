---
id: E4-003
epic: EPIC-004
title: Persist Matter metadata and long-running operations durably
status: done
priority: critical
depends_on: [E4-002]
adrs: [ADR-0007, ADR-0008, ADR-0014, ADR-0035, ADR-0036, ADR-0037]
created: 2026-07-12
updated: 2026-07-12
---

# E4-003: Matter Storage

## Outcome

SQLite and `SecretStore` boundaries preserve stable Matter identity, operation
progress, command convergence, authorization consumption, and repair state
across process restart without persisting plaintext secrets.

## Tasks

- [x] Add an application-owned Matter repository port with transaction-scoped
  operations and explicit optimistic revisions.
- [x] Add a forward-only migration for fabrics, nodes, endpoints, projections,
  subscriptions, operations, progress facts, and repair records.
- [x] Persist desired/reported state, report versions, freshness, convergence,
  and descriptor/projection revisions.
- [x] Persist only opaque `SecretRef` values for live fabric material.
- [x] Persist unlock authorization bindings, expiry, consumption, and decision
  facts without storing bearer material.
- [x] Atomically supersede undispatched desired-state commands while retaining
  the cancelled command and replacement relation.
- [x] Add restart queries for incomplete operations, stale subscriptions,
  unresolved convergence, and repair-required resources.
- [x] Add bounded retention that protects active operations, current identity,
  unresolved repair, current state, and unexpired authorization facts.
- [x] Add migration fixtures for old, current, interrupted, and malformed states.

## Acceptance criteria

- [x] A process can recover every non-terminal Matter operation deterministically.
- [x] Duplicate node/endpoint identities cannot arise from address or session
  changes.
- [x] Unlock authorization is consumed atomically at most once.
- [x] A crash cannot hide a supersession, dispatch decision, or partial cleanup.
- [x] Database backup and diagnostics contain no Matter secret values.

## Verification

- [x] Fresh, upgrade, reopen, rollback-on-error, and concurrent-writer tests pass.
- [x] Operation and authorization transition property tests pass.
- [x] Restart query fixtures cover every non-terminal phase.
- [x] Secret canaries are absent from database, backups, errors, and diagnostics.
- [x] Retention never removes protected rows or breaks foreign-key integrity.

## Evidence

- `homemagic-application::MatterRepository` owns object-safe async contracts for
  fabrics, nodes, projections, subscriptions, operations, repair, unlock
  authorization, desired-state slots, dispatch markers, recovery, and retention.
- `0006_matter_controller.sql` uses stable fabric/node/endpoint/projection keys;
  no address, session, SDK handle, or secret value participates in identity.
- [Matter Storage Boundary](../../architecture/matter-storage.md) documents
  durable ownership, transaction invariants, recovery, secret handling, and
  retention.
- `matter_repository_contract.rs` passes eight restart and safety scenarios,
  including every nonterminal operation phase, pending projection/subscription
  recovery, rollback, concurrent one-time authorization, expiry, supersession,
  dispatch, malformed state, retention, and live/backup secret canaries.
- Migration fixtures pass for empty, historical, and explicit schema-5 states;
  schema 6 reopens with integrity `ok`.
- Workspace format, strict Clippy, all tests/features, warning-denied Rustdoc,
  Matter dependency boundaries, secret scan, and patch hygiene passed on
  2026-07-12.

## Progress log

- 2026-07-12: Planned independently of E4-004 after E4-002.
- 2026-07-12: Completed the application-owned repository, forward-only schema 6
  migration, atomic operation/repair and command-convergence writes, single-use
  unlock authorization, deterministic restart queries, protected retention, and
  the full storage contract evidence. E4-004 remains ready.
