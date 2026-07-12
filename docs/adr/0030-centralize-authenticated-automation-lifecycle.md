# ADR-0030: Centralize authenticated automation lifecycle operations

- Status: Accepted
- Date: 2026-07-12

## Context

RPC handlers must not independently reconstruct governance transitions. Doing
so would create different behavior for internal callers, future MCP tools, and
JSON-RPC, and could allow a transport parameter to impersonate an author.

Validation, simulation, approval, and activation are multi-step evidence
operations over exact immutable content. Their actor and hash checks belong at
an application boundary below transport parsing.

## Decision

`AutomationLifecycleService` is the single authenticated boundary for draft
authoring, validation, deterministic simulation, approval decisions, and exact
activation. Every method receives an authenticated `Actor`; author identity is
read from that value and must match document provenance. Transport callers can
never supply a substitute actor ID.

Validation compiles the current optimistic draft against one foundation
snapshot and persists exact document, plan, and registry evidence. Simulation
accepts synthetic history but constructs compiler-owned plan, run, occurrence,
and correlation identities internally. A successful simulation advances a
version to `ready` for `activation_grant`, or `awaiting_approval` for
`explicit_user_approval`.

Approval records bind actor, immutable version, document hash, and plan hash.
Activation accepts only a ready version and sends those exact hashes plus the
expected identity revision to the atomic repository operation.

## Consequences

- JSON-RPC and internal/MCP callers can share identical governance behavior.
- Agents do not construct internal simulation IDs.
- Actor ownership, optimistic conflicts, and exact evidence are enforced below
  transport code.
- Query pagination, operational transitions, run operations, and event emission
  remain to be added around this boundary in E3-007.
