---
id: E4-007
epic: EPIC-004
title: Expose simulator-backed durable Matter workflows over RPC
status: done
priority: high
depends_on: [E4-003, E4-005, E4-006]
adrs: [ADR-0003, ADR-0012, ADR-0013, ADR-0016, ADR-0033, ADR-0035, ADR-0037]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007: Matter RPC Workflows

## Child issues

| Issue | Status | Outcome |
| --- | --- | --- |
| [E4-007-01](E4-007-01-administration-service.md) | Done | One authenticated durable administration boundary |
| [E4-007-02](E4-007-02-fabric-workflows.md) | Done | Fabric status, create, simulated export, and restore |
| [E4-007-03](E4-007-03-node-operation-workflows.md) | Done | Commissioning, removal, cancellation, and restart recovery |
| [E4-007-04](E4-007-04-subscription-diagnostics-repair.md) | Done | Bounded diagnostics and explicit subscription repair |
| [E4-007-05](E4-007-05-authenticated-rpc-events.md) | Done | Versioned `matter.*` RPC and actor-filtered operation events |

## Outcome

Authenticated callers can manage a simulated fabric, commission and remove
nodes, inspect operations and diagnostics, repair subscriptions, and authorize
an exact unlock through durable RPC workflows while normal device behavior stays
capability-oriented.

## Tasks

- [x] Add one authenticated application service shared by internal and JSON-RPC
  callers for every Matter administration mutation.
- [x] Implement durable fabric status/create and simulated export/restore
  workflows with explicit evidence labels.
- [x] Implement commissioning start, cancel, get, list, restart recovery, and
  repair-required handling.
- [x] Implement node list/get/remove and partial-cleanup reporting.
- [x] Implement subscription status and explicit repair workflows.
- [x] Implement bounded redacted controller/fabric/node/endpoint diagnostics.
- [x] Implement interactive unlock-authorization creation with server-derived
  actor and policy context.
- [x] Finalize versioned JSON-RPC schemas and stable error mappings for the
  `matter.*` administration method group.
- [x] Return operation envelopes immediately for long-running mutations.
- [x] Stream actor-filtered operation transitions through the durable event
  cursor without exposing secret input or bearer authorization material.
- [x] Keep normal state and action access on common device and command methods.
- [x] Document sensitive-input handling, idempotency, cancellation, restart, and
  repair procedures.

## Acceptance criteria

- [x] Actor identity and authorization context are never accepted from params.
- [x] Setup codes and sensitive export/restore input never enter logs, events,
  operation details, or ordinary request hashes.
- [x] Restart in every simulated phase yields completed, failed, cancelled, or
  explicit `repair_required`, never silent disappearance.
- [x] Raw cluster/attribute writes are absent from public RPC schemas.
- [x] The same common command RPC controls simulated light and lock capabilities.

## Verification

- [x] SQLite-backed JSON-RPC happy, invalid, conflict, unauthorized, and restart
  matrices pass.
- [x] Actor isolation and event-cursor reconnect tests pass.
- [x] Sensitive input and diagnostic secret-canary scans pass.
- [x] Partial commissioning/removal cleanup remains queryable and repairable.
- [x] API examples and operator procedures match executable schemas.

## Progress log

- 2026-07-12: Planned as the completion gate for simulator-backed Track A.
- 2026-07-12: E4-006 completed governed commands and unlock approval. This
  issue was decomposed into five dependency-ordered child issues; E4-007-01 is
  ready.
- 2026-07-12: E4-007-01 completed authenticated, exact-grant, idempotent Matter
  administration admission and durable operation bindings. E4-007-02 is ready.
- 2026-07-12: E4-007-02 fabric workflows are implemented locally. Targeted
  contracts, exact CI-format Clippy, boundary/secret scans, and the full
  privileged workspace test suite pass. Commit, push, public CI, and issue
  closure remain pending.
- 2026-07-12: Public CI run `29202622965` verified E4-007-02 on Linux x86_64
  and macOS ARM64. E4-007-02 is done and E4-007-03 is ready.
- 2026-07-12: E4-007-03 completed durable commissioning, cancellation, bounded
  inventory, phase-by-phase restart recovery, and node removal with visible
  partial cleanup. Public CI run `29205464608` passed; E4-007-04 is ready.
- 2026-07-12: E4-007-04 completed bounded read-only diagnostics, durable
  subscription health, explicit repair, and fail-closed restart reconciliation.
  Public CI run `29207369049` passed; E4-007-05 is ready.
- 2026-07-12: E4-007-05 was decomposed under ADR-0042; E4-007-05-01 read RPC
  contracts are ready.
- 2026-07-12: E4-007-05 completed authenticated reads, immediate mutation
  admission, isolated sensitive exchange, actor-scoped durable events, and
  executable Track A evidence. Final public CI run `29209289949` passed; E4-007
  is done.
