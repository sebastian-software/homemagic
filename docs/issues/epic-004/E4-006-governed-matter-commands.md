---
id: E4-006
epic: EPIC-004
title: Govern Matter commands and interactive unlock authorization
status: planned
priority: critical
depends_on: [E4-003, E4-004, E4-005]
adrs: [ADR-0014, ADR-0015, ADR-0016, ADR-0035, ADR-0036]
created: 2026-07-12
updated: 2026-07-12
---

# E4-006: Governed Matter Commands

## Outcome

Common light and lock commands use the shared durable command control plane,
collapse obsolete undispatched desired states, converge honestly after dispatch,
and cannot unlock without exact interactive authorization.

## Tasks

- [ ] Implement Matter `CommandDispatcher` and `CommandConfirmation` adapters
  over the SDK-neutral controller port.
- [ ] Translate only supported common capability payloads into adapter-private
  protocol invocations.
- [ ] Treat controller acknowledgement separately from reported confirmation.
- [ ] Add a shared desired-state slot keyed by device endpoint and capability.
- [ ] Atomically cancel and link older undispatched commands when a newer desired
  revision supersedes them.
- [ ] Never retract or blindly resend a command that reached `dispatched`.
- [ ] Reconcile an in-flight or indeterminate outcome toward the latest desired
  revision using observation and bounded reads.
- [ ] Mint unlock authorization only through an authenticated interactive user
  action with explicit authority.
- [ ] Bind authorization to requester actor, exact lock target, `unlock` action,
  desired revision, policy revision, expiry, and single-use nonce/reference.
- [ ] Consume authorization atomically immediately before dispatch and reject
  missing, expired, reused, mismatched, or policy-stale authorizations.
- [ ] Prevent automations, agents, broad space grants, and adapter code from
  minting or widening unlock authorization.
- [ ] Extend the command threat model and recovery documentation.

## Acceptance criteria

- [ ] `on -> off -> on` emits one `on` when all requests remain undispatched.
- [ ] After dispatch, history preserves intermediate facts and reconciliation
  targets the latest desired state without claiming no physical transition.
- [ ] `lock` follows exact-target command policy without the extra interactive
  authorization.
- [ ] `unlock` requires both normal exact-target policy and one valid interactive
  authorization for that revision.
- [ ] No Matter-specific path can bypass command persistence, policy, deadlines,
  idempotency, audit, or restart recovery.

## Verification

- [ ] Dispatch-barrier tests cover supersession before and after dispatch.
- [ ] Acknowledgement/observation mismatch and timeout tests pass.
- [ ] Every unlock authorization rejection reason and one valid path are tested.
- [ ] Concurrent consumption proves at-most-once authorization use.
- [ ] Automation and internal-caller bypass attempts are rejected.
- [ ] Restart tests never duplicate an indeterminate physical invocation.

## Progress log

- 2026-07-12: Planned with interactive authorization for unlock only.
