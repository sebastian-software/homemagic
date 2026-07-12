---
id: E4-007-03-01
epic: EPIC-004
parent: E4-007-03
title: Admit fabric-scoped commissioning without persisting setup input
status: done
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

- [x] Add the ADR-0040 fabric, operation, and node target semantics.
- [x] Restrict commissioning admission to an active installation fabric.
- [x] Introduce a non-serializable, redacted commissioning input type.
- [x] Return the durable `requested` operation before accepting setup bytes.
- [x] Keep setup bytes out of canonical hashes, SQLite, events, and diagnostics.
- [x] Persist an explicit operation-to-node result contract for later success.

## Acceptance criteria

- [x] Commissioning admission never accepts or fabricates a node ID.
- [x] Equivalent actor/fabric/idempotency retries return the same operation.
- [x] Conflicting key reuse returns the original operation ID without work.
- [x] Setup payload ownership ends at the sensitive controller request.

## Verification

- [x] Admission allowed, denied, duplicate, conflict, and inactive-fabric tests
  pass.
- [x] Setup canaries remain absent from database, WAL, debug, events, and hashes.
- [x] Full local workspace gates pass.
- [x] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: Implemented ADR-0040 targets, fabric-scoped actor-bound admission,
  redacted non-serializable setup input, and schema 10 operation-to-node result
  identity. Twenty-nine Matter repository contracts, nine migration fixtures,
  the full workspace suite, strict Clippy, boundary checks, and secret scans pass
  locally. Commit, push, and public CI remain pending.
- 2026-07-12: Commits `857b72d` and `6676fbb` were pushed to `main`. Public CI
  run `29203093982` passed Linux x86_64 quality and simulator hashes on Linux
  x86_64 and macOS ARM64. This child issue is done.
