---
id: E4-007-03-01
epic: EPIC-004
parent: E4-007-03
title: Admit fabric-scoped commissioning without persisting setup input
status: ready
priority: high
depends_on: [E4-007-02]
adrs: [ADR-0013, ADR-0014, ADR-0033, ADR-0037, ADR-0040]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-03-01: Commissioning Target and Admission

## Outcome

An authenticated actor admits an idempotent commissioning attempt against the
active installation fabric before providing sensitive setup input. No invented
node identity or setup byte becomes an ordinary durable fact.

## Tasks

- [ ] Add the ADR-0040 fabric, operation, and node target semantics.
- [ ] Restrict commissioning admission to an active installation fabric.
- [ ] Introduce a non-serializable, redacted commissioning input type.
- [ ] Return the durable `requested` operation before accepting setup bytes.
- [ ] Keep setup bytes out of canonical hashes, SQLite, events, and diagnostics.
- [ ] Persist an explicit operation-to-node result contract for later success.

## Acceptance criteria

- [ ] Commissioning admission never accepts or fabricates a node ID.
- [ ] Equivalent actor/fabric/idempotency retries return the same operation.
- [ ] Conflicting key reuse returns the original operation ID without work.
- [ ] Setup payload ownership ends at the sensitive controller request.

## Verification

- [ ] Admission allowed, denied, duplicate, conflict, and inactive-fabric tests
  pass.
- [ ] Setup canaries remain absent from database, WAL, debug, events, and hashes.
