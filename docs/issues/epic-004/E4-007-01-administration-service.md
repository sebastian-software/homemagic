---
id: E4-007-01
epic: EPIC-004
parent: E4-007
title: Establish the authenticated Matter administration service
status: ready
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

- [ ] Define typed administration requests and results without transport or SDK
  types.
- [ ] Derive actor and installation context from authenticated durable state.
- [ ] Add explicit administration actions and default-deny grants.
- [ ] Create or resume one durable operation before every controller mutation.
- [ ] Make retries idempotent and return current operation envelopes.
- [ ] Normalize controller failures into durable failed or repair-required
  transitions without sensitive detail.
- [ ] Expose bounded operation get/list/cancel application methods.

## Acceptance criteria

- [ ] No mutation accepts actor, installation, operation phase, or controller
  identity from untrusted parameters.
- [ ] Internal and RPC callers must use the same service.
- [ ] A crash after persistence leaves queryable restart work.
- [ ] Duplicate mutation requests cannot create duplicate physical work.

## Verification

- [ ] Service contract tests cover allowed, denied, duplicate, conflict,
  cancellation, and restart paths.
- [ ] Persisted requests, audits, and errors contain no sensitive input.
