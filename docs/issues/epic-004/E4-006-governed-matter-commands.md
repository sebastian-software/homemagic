---
id: E4-006
epic: EPIC-004
title: Govern Matter commands and interactive unlock authorization
status: done
priority: critical
depends_on: [E4-003, E4-004, E4-005]
adrs: [ADR-0014, ADR-0015, ADR-0016, ADR-0035, ADR-0036]
created: 2026-07-12
updated: 2026-07-12
---

# E4-006: Governed Matter Commands

## Child issues

| Issue | Status | Outcome |
| --- | --- | --- |
| [E4-006-01](E4-006-01-access-control-command-contract.md) | Done | Typed access-control commands and explicit user approval authority |
| [E4-006-02](E4-006-02-desired-state-supersession.md) | Done | Shared monotonic desired slot and pre-dispatch supersession |
| [E4-006-03](E4-006-03-matter-command-adapters.md) | Done | SDK-neutral dispatch and observation-only confirmation |
| [E4-006-04](E4-006-04-interactive-unlock-authorization.md) | Done | Exact sixty-second single-use unlock authorization |

## Outcome

Common light and lock commands use the shared durable command control plane,
collapse obsolete undispatched desired states, converge honestly after dispatch,
and cannot unlock without exact interactive authorization.

## Tasks

- [x] Implement Matter `CommandDispatcher` and `CommandConfirmation` adapters
  over the SDK-neutral controller port.
- [x] Translate only supported common capability payloads into adapter-private
  protocol invocations.
- [x] Treat controller acknowledgement separately from reported confirmation.
- [x] Add a shared desired-state slot keyed by device endpoint and capability.
- [x] Atomically cancel and link older undispatched commands when a newer desired
  revision supersedes them.
- [x] Never retract or blindly resend a command that reached `dispatched`.
- [x] Reconcile an in-flight or indeterminate outcome toward the latest desired
  revision using observation and bounded reads.
- [x] Mint unlock authorization only through an authenticated interactive user
  action with explicit authority.
- [x] Bind authorization to requester actor, exact lock target, `unlock` action,
  desired revision, policy revision, expiry, and single-use nonce/reference.
- [x] Consume authorization atomically immediately before dispatch and reject
  missing, expired, reused, mismatched, or policy-stale authorizations.
- [x] Prevent automations, agents, broad space grants, and adapter code from
  minting or widening unlock authorization.
- [x] Extend the command threat model and recovery documentation.

## Acceptance criteria

- [x] `on -> off -> on` emits one `on` when all requests remain undispatched.
- [x] After dispatch, history preserves intermediate facts and reconciliation
  targets the latest desired state without claiming no physical transition.
- [x] `lock` follows exact-target command policy without the extra interactive
  authorization.
- [x] `unlock` requires both normal exact-target policy and one valid interactive
  authorization for that revision.
- [x] No Matter-specific path can bypass command persistence, policy, deadlines,
  idempotency, audit, or restart recovery.

## Verification

- [x] Dispatch-barrier tests cover supersession before and after dispatch.
- [x] Acknowledgement/observation mismatch and timeout tests pass.
- [x] Every unlock authorization rejection class and one valid path are tested.
- [x] Concurrent consumption proves at-most-once authorization use.
- [x] Automation and internal-caller bypass attempts are rejected.
- [x] Restart tests never duplicate an indeterminate physical invocation.

## Progress log

- 2026-07-12: Planned with interactive authorization for unlock only.
- 2026-07-12: E4-005 completed stable projection and subscription recovery;
  this issue is ready.
- 2026-07-12: Decomposed implementation into four dependency-ordered child
  issues. E4-006-01 is ready.
- 2026-07-12: Completed all four child issues: governed typed Matter adapters,
  monotonic desired-state supersession, observation-only confirmation, and
  exact user-only unlock approval with atomic single-use dispatch admission.
