---
id: E4-007-05-02
epic: EPIC-004
parent: E4-007-05
title: Admit Matter mutations and isolate sensitive exchange
status: in_progress
priority: high
depends_on: [E4-007-05-01]
adrs: [ADR-0013, ADR-0014, ADR-0035, ADR-0037, ADR-0042]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-05-02: Sensitive Mutations

## Outcome

Ordinary mutation admission returns durable operation envelopes immediately;
commissioning setup, protected export, and restore bytes cross only the
dedicated sensitive endpoint, and unlock approval delegates to the governed
common command service.

## Tasks

- [x] Publish create, commission, cancel, remove, repair, and operation methods.
- [x] Return `matter.operation.v1` immediately after durable admission.
- [x] Add `/rpc/sensitive` with an explicit setup/export/restore allowlist.
- [x] Disable body/param tracing and convert bytes immediately to `SecretValue`.
- [x] Route exact unlock approval through `CommandService::approve_unlock`.
- [x] Wake daemon-owned operation execution without transport-owned tasks.

## Acceptance criteria

- [x] Sensitive input never appears in hashes, events, errors, logs, or SQLite.
- [x] Ordinary methods cannot submit or retrieve sensitive values.
- [x] No public RPC exposes raw cluster writes or workflow `run` internals.

## Verification

- [x] Success, duplicate, conflict, denied, timeout, restart, and canary tests pass.
- [x] Common `commands.execute` still controls simulated light and lock state.
- [ ] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: Commit `d08668c` added strict ordinary mutation admission,
  dedicated sensitive exchange, a bounded daemon-owned execution handoff,
  governed unlock delegation, executable schemas, and SQLite canary coverage.
  Full local workspace tests, strict Clippy, Matter boundaries, and secret scans
  pass; public CI remains pending.
