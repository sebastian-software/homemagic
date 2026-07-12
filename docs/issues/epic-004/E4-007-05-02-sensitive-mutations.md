---
id: E4-007-05-02
epic: EPIC-004
parent: E4-007-05
title: Admit Matter mutations and isolate sensitive exchange
status: ready
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

- [ ] Publish create, commission, cancel, remove, repair, and operation methods.
- [ ] Return `matter.operation.v1` immediately after durable admission.
- [ ] Add `/rpc/sensitive` with an explicit setup/export/restore allowlist.
- [ ] Disable body/param tracing and convert bytes immediately to `SecretValue`.
- [ ] Route exact unlock approval through `CommandService::approve_unlock`.
- [ ] Wake daemon-owned operation execution without transport-owned tasks.

## Acceptance criteria

- [ ] Sensitive input never appears in hashes, events, errors, logs, or SQLite.
- [ ] Ordinary methods cannot submit or retrieve sensitive values.
- [ ] No public RPC exposes raw cluster writes or workflow `run` internals.

## Verification

- [ ] Success, duplicate, conflict, denied, timeout, restart, and canary tests pass.
- [ ] Common `commands.execute` still controls simulated light and lock state.
