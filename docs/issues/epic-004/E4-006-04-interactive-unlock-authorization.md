---
id: E4-006-04
epic: EPIC-004
parent: E4-006
title: Enforce exact interactive single-use unlock authorization
status: planned
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

- [ ] Persist canonical request hash, action, exact target, desired revision,
  policy revision, requester, approver, issue time, and hard expiry.
- [ ] Expose a dedicated interactive approval application method; do not add an
  authorization field to the normal command request.
- [ ] Reject non-user principals and non-exact approval grants.
- [ ] Revalidate command state, target, projection freshness, desired slot,
  request binding, policy revision, expiry, and unused state immediately before
  dispatch.
- [ ] Atomically consume authorization, transition command to dispatched, append
  audit, and mark the desired slot dispatched.
- [ ] Invalidate unused authorization on supersession, cancellation, target or
  projection change, policy change, expiry, or terminal outcome.
- [ ] Keep authorization identifiers out of events and logs.

## Acceptance criteria

- [ ] Lock dispatches with normal exact-target security policy only.
- [ ] Unlock cannot dispatch without one exact interactive authorization.
- [ ] Concurrent consumers produce exactly one dispatch admission.
- [ ] Automation, agent, adapter, service, broad-grant, expired, reused,
  mismatched, and policy-stale attempts all fail closed.

## Verification

- [ ] Every rejection reason and one valid path have contract tests.
- [ ] Concurrent SQLite consumption proves at-most-once admission.
- [ ] Restart preserves audit facts but never extends expiry or redispatches.
- [ ] Threat-model and recovery documentation match executable behavior.

