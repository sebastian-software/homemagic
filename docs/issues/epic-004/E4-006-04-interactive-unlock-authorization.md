---
id: E4-006-04
epic: EPIC-004
parent: E4-006
title: Enforce exact interactive single-use unlock authorization
status: done
priority: critical
depends_on: [E4-006-01, E4-006-02, E4-006-03]
adrs: [ADR-0014, ADR-0015, ADR-0035, ADR-0036]
created: 2026-07-12
updated: 2026-07-12
---

# E4-006-04: Interactive Unlock Authorization

## Outcome

An unlock remains validated but undispatched until an authenticated interactive
user with exact approval authority authorizes that command, target, request,
desired revision, and policy revision for one use within sixty seconds.

## Tasks

- [x] Persist canonical request hash, action, exact target, desired revision,
  policy revision, requester, approver, issue time, and hard expiry.
- [x] Expose a dedicated interactive approval application method; do not add an
  authorization field to the normal command request.
- [x] Reject non-user principals and non-exact approval grants.
- [x] Revalidate command state, target, projection freshness, desired slot,
  request binding, policy revision, expiry, and unused state immediately before
  dispatch.
- [x] Atomically consume authorization, transition command to dispatched, append
  audit, and mark the desired slot dispatched.
- [x] Invalidate unused authorization on supersession, cancellation, target or
  projection change, policy change, expiry, or terminal outcome.
- [x] Keep authorization identifiers out of events and logs.

## Acceptance criteria

- [x] Lock dispatches with normal exact-target security policy only.
- [x] Unlock cannot dispatch without one exact interactive authorization.
- [x] Concurrent consumers produce exactly one dispatch admission.
- [x] Automation, agent, adapter, service, broad-grant, expired, reused,
  mismatched, and policy-stale attempts all fail closed.

## Verification

- [x] Every rejection class and one valid path have contract tests.
- [x] Concurrent SQLite consumption proves at-most-once admission.
- [x] Restart preserves audit facts but never extends expiry or redispatches.
- [x] Threat-model and recovery documentation match executable behavior.

## Progress log

- 2026-07-12: Added the dedicated `approve_unlock` application path, exact
  user-only policy, immutable sixty-second authorization bindings, and atomic
  authorization consumption plus dispatch. End-to-end tests prove no adapter
  call before approval, one call after approval, fail-closed stale bindings,
  and concurrent at-most-once admission.
