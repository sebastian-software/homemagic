---
id: E4-007-01
epic: EPIC-004
parent: E4-007
title: Establish the authenticated Matter administration service
status: done
priority: high
depends_on: [E4-003, E4-005, E4-006]
adrs: [ADR-0012, ADR-0013, ADR-0033, ADR-0037]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-01: Matter Administration Service

## Outcome

One application-owned service authenticates and authorizes every Matter
administration mutation, persists operation identity before controller work,
and exposes bounded reads shared by internal and future RPC callers.

## Tasks

- [x] Define typed administration requests and results without transport or SDK
  types.
- [x] Derive actor and installation context from authenticated durable state.
- [x] Add explicit administration actions and default-deny grants.
- [x] Create or resume one durable operation before every controller mutation.
- [x] Make retries idempotent and return current operation envelopes.
- [x] Normalize controller failures into durable failed or repair-required
  transitions without sensitive detail.
- [x] Expose bounded operation get/list/cancel application methods.

## Acceptance criteria

- [x] No mutation accepts actor, installation, operation phase, or controller
  identity from untrusted parameters.
- [x] Internal workflows and the later RPC binding have one public application
  service contract; transport wiring remains E4-007-05.
- [x] A crash after persistence leaves queryable restart work.
- [x] Duplicate mutation requests cannot create duplicate physical work.

## Verification

- [x] Service contract tests cover allowed, denied, duplicate, conflict,
  cancellation, and restart paths.
- [x] Persisted requests, audits, and errors contain no sensitive input.

## Progress log

- 2026-07-12: Implemented typed actor-bound administration admission, eight
  independently grantable actions, schema 8 operation bindings, canonical
  actor-scoped idempotency, bounded owner reads, safe pre-controller
  cancellation, structured failure/repair normalization, and an explicit CLI
  grant workflow. SQLite contracts and all local workspace gates pass.
- 2026-07-12: Public CI run `29199747179` passed Linux x86_64 quality and the
  deterministic simulator hash on Linux x86_64 and macOS ARM64.
