---
id: E4-007
epic: EPIC-004
title: Expose simulator-backed durable Matter workflows over RPC
status: planned
priority: high
depends_on: [E4-003, E4-005, E4-006]
adrs: [ADR-0003, ADR-0012, ADR-0013, ADR-0016, ADR-0033, ADR-0035, ADR-0037]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007: Matter RPC Workflows

## Outcome

Authenticated callers can manage a simulated fabric, commission and remove
nodes, inspect operations and diagnostics, repair subscriptions, and authorize
an exact unlock through durable RPC workflows while normal device behavior stays
capability-oriented.

## Tasks

- [ ] Add one authenticated application service shared by internal and JSON-RPC
  callers for every Matter administration mutation.
- [ ] Implement durable fabric status/create and simulated export/restore
  workflows with explicit evidence labels.
- [ ] Implement commissioning start, cancel, get, list, restart recovery, and
  repair-required handling.
- [ ] Implement node list/get/remove and partial-cleanup reporting.
- [ ] Implement subscription status and explicit repair workflows.
- [ ] Implement bounded redacted controller/fabric/node/endpoint diagnostics.
- [ ] Implement interactive unlock-authorization creation with server-derived
  actor and policy context.
- [ ] Finalize versioned JSON-RPC schemas and stable error mappings for the
  `matter.*` administration method group.
- [ ] Return operation envelopes immediately for long-running mutations.
- [ ] Stream actor-filtered operation transitions through the durable event
  cursor without exposing secret input or bearer authorization material.
- [ ] Keep normal state and action access on common device and command methods.
- [ ] Document sensitive-input handling, idempotency, cancellation, restart, and
  repair procedures.

## Acceptance criteria

- [ ] Actor identity and authorization context are never accepted from params.
- [ ] Setup codes and sensitive export/restore input never enter logs, events,
  operation details, or ordinary request hashes.
- [ ] Restart in every simulated phase yields completed, failed, cancelled, or
  explicit `repair_required`, never silent disappearance.
- [ ] Raw cluster/attribute writes are absent from public RPC schemas.
- [ ] The same common command RPC controls simulated light and lock capabilities.

## Verification

- [ ] SQLite-backed JSON-RPC happy, invalid, conflict, unauthorized, and restart
  matrices pass.
- [ ] Actor isolation and event-cursor reconnect tests pass.
- [ ] Sensitive input and diagnostic secret-canary scans pass.
- [ ] Partial commissioning/removal cleanup remains queryable and repairable.
- [ ] API examples and operator procedures match executable schemas.

## Progress log

- 2026-07-12: Planned as the completion gate for simulator-backed Track A.
