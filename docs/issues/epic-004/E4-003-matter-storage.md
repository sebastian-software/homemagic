---
id: E4-003
epic: EPIC-004
title: Persist Matter metadata and long-running operations durably
status: ready
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

- [ ] Add an application-owned Matter repository port with transaction-scoped
  operations and explicit optimistic revisions.
- [ ] Add a forward-only migration for fabrics, nodes, endpoints, projections,
  subscriptions, operations, progress facts, and repair records.
- [ ] Persist desired/reported state, report versions, freshness, convergence,
  and descriptor/projection revisions.
- [ ] Persist only opaque `SecretRef` values for live fabric material.
- [ ] Persist unlock authorization bindings, expiry, consumption, and decision
  facts without storing bearer material.
- [ ] Atomically supersede undispatched desired-state commands while retaining
  the cancelled command and replacement relation.
- [ ] Add restart queries for incomplete operations, stale subscriptions,
  unresolved convergence, and repair-required resources.
- [ ] Add bounded retention that protects active operations, current identity,
  unresolved repair, current state, and unexpired authorization facts.
- [ ] Add migration fixtures for old, current, interrupted, and malformed states.

## Acceptance criteria

- [ ] A process can recover every non-terminal Matter operation deterministically.
- [ ] Duplicate node/endpoint identities cannot arise from address or session
  changes.
- [ ] Unlock authorization is consumed atomically at most once.
- [ ] A crash cannot hide a supersession, dispatch decision, or partial cleanup.
- [ ] Database backup and diagnostics contain no Matter secret values.

## Verification

- [ ] Fresh, upgrade, reopen, rollback-on-error, and concurrent-writer tests pass.
- [ ] Operation and authorization transition property tests pass.
- [ ] Restart query fixtures cover every non-terminal phase.
- [ ] Secret canaries are absent from database, logs, errors, and diagnostics.
- [ ] Retention never removes protected rows or breaks foreign-key integrity.

## Progress log

- 2026-07-12: Planned independently of E4-004 after E4-002.
