# ADR-0035: Require exact interactive authorization for every unlock

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0013, ADR-0014, ADR-0015, ADR-0019, EPIC-004, E4-001

## Context

Unlocking changes access to a physical space. An exact command grant remains
necessary, but standing agent, automation, or space-level authority should not
silently permit a future unlock. Approval of an automation version governs that
version's activation; it is not proof that a user intends a particular door to
unlock now.

The authorization must remain compatible with durable commands, retries, actor
authentication, desired-state supersession, and restart recovery.

## Decision

Every `unlock` is first persisted and validated as a normal ADR-0014 command. It
receives a server-generated desired-state revision but remains undispatched while
awaiting a separate interactive authorization fact.

Only an authenticated user actor with explicit unlock-approval authority may
authorize it through an interactive session. Agents, automations, adapters, and
internal background tasks cannot mint authorization.

The authorization is bound to:

- command ID and canonical request hash;
- authenticated requesting actor;
- exact device, endpoint, capability, and `unlock` action;
- desired-state and policy revisions;
- approving user actor and issue time;
- a 60-second expiry and one server-generated opaque authorization ID.

The authorization record contains no reusable credential. The opaque ID is
unpredictable, actor-bound, redacted from events/logs, and insufficient after
expiry or any binding change.

Immediately before adapter dispatch, one repository transaction revalidates the
command, actor grant, policy revision, target/projection freshness, desired
revision, expiry, and unused authorization, then consumes the authorization and
permits transition to `dispatched`. Concurrent attempts can consume it at most
once.

Supersession, cancellation, target/projection change, policy change, expiry, or
terminal command outcome invalidates any unused authorization. There is no
automatic renewal, retry authorization, standing approval, or approval inherited
from an automation version. A new unlock command or changed revision needs a new
interactive authorization.

`lock` remains security-classified and requires the exact capability/target
grant and normal command policy, but it does not require this extra interactive
authorization.

## Consequences

- A compromised or over-broad agent grant cannot silently unlock a door later.
- Users must confirm each exact unlock within a short window.
- Unlock RPC and UI/MCP flows need an explicit pending-authorization state.
- Restart can preserve the audit fact but cannot extend its expiry.
- Physical validation still requires separate test-time authorization; a
  simulated authorization is not consent to operate hardware.
