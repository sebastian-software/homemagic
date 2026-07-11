# ADR-0015: Evaluate a default-deny risk policy before dispatch

- Status: Accepted
- Date: 2026-07-11

## Context

The same command may be harmless for a lamp, mechanically risky for a cover, or
security-sensitive for a lock. RPC, MCP, automations, and internal calls must not
develop separate authorization rules.

## Decision

The application command service evaluates one deterministic, versioned policy
before persistence can transition to `dispatched`. Policy input includes actor,
action, target, spaces, capability schema, risk class, current freshness,
constraints, and dry-run status. The complete allow/deny decision and stable
reason codes are persisted.

Policy is default-deny:

- observe-only access does not imply command access;
- comfort commands require an explicit execute grant matching capability and
  target or space;
- mechanical commands additionally require an explicit mechanical grant, fresh
  state, supported/calibrated constraints, and per-device serialization;
- security commands require an explicit exact capability/target grant and are
  not enabled by broad space grants;
- rate and concurrency limits apply per actor and device;
- dry runs execute identical authentication, validation, and policy logic but
  cannot dispatch.

Adapters receive only validated dispatch requests. They cannot relax policy or
offer a public raw-command escape hatch.

## Consequences

- Denials are explainable and transport-independent.
- Agent-authored automations can use the same governed command path later.
- Initial operator setup must create narrowly scoped grants before commands work.
