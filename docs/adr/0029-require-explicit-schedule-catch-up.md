# ADR-0029: Require an explicit request for schedule catch-up

- Status: Accepted
- Date: 2026-07-12

## Context

Starting the controller after downtime must not produce a burst of stale
physical actions. At the same time, an operator or agent may intentionally want
to execute one specific schedule occurrence that was missed.

A generic replay-from-time API would hide which action was selected, could
expand as schedules change, and would be difficult to make idempotent.

## Decision

Catch-up targets one exact active automation and one UTC instant. The scheduler
first proves that the instant belongs to an active validated schedule and that
its normal occurrence window has elapsed. It then persists:

1. the deterministic original schedule occurrence as `missed_skipped`; and
2. a separate scheduled occurrence containing `AutomationCatchUp` evidence.

Catch-up evidence retains the missed occurrence ID, authenticated requesting
actor, actor-scoped idempotency key, and request time. The new occurrence ID is
derived from all stable request identities, so retrying the same request returns
the same durable occurrence. Admission and execution then follow normal run
mode, policy, and command-service rules.

The operation rejects inactive automations, instants that are not exact schedule
occurrences, and occurrences whose normal window is still open. Startup never
calls this operation and never scans downtime for work to replay.

## Consequences

- No missed schedule can execute merely because the process restarted.
- Every exceptional catch-up is separately attributable and inspectable.
- A caller must make one concrete choice instead of requesting ambiguous bulk
  replay.
- The RPC surface still needs to authenticate and expose this application
  operation before agents can invoke it remotely.
