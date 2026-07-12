# ADR-0042: Separate Matter RPC, sensitive exchange, and operation events

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0003, ADR-0012, ADR-0013, ADR-0014, ADR-0016, ADR-0035,
  ADR-0037, ADR-0041, EPIC-004, E4-007-05

## Context

Matter administration combines ordinary bounded reads, long-running durable
operations, and a small set of sensitive values: commissioning setup payloads,
protected fabric envelopes, and one-time recovery material. Treating every
value as ordinary JSON-RPC params or events would make logging, replay,
idempotency, and cursor retention unsafe. Running long mutations inside the HTTP
request would also make timeout and restart behavior transport-dependent.

Operation events must be reconnectable while remaining actor-scoped. Filtering
only at initial subscription time is insufficient because the general event log
is shared by an installation and contains work admitted for different actors.

## Decision

Ordinary versioned `matter.*` methods use the authenticated `/rpc` endpoint.
They accept no actor, grant, policy, controller, cluster, attribute, or raw
command context. Read methods return bounded redacted DTOs. Mutation admission
returns a `matter.operation.v1` envelope immediately after the requested
operation and immutable actor binding are durable. A daemon-owned worker, not
the transport request, advances eligible operations.

Sensitive setup, protected export delivery, and restore material use an
authenticated `/rpc/sensitive` JSON-RPC endpoint with an explicit method
allowlist and tracing that never records bodies or params. Sensitive values are
converted immediately into non-serializable `SecretValue` inputs, are never
included in ordinary request hashes, operation payloads, errors, or events, and
are never replayed after process loss. An operation that cannot continue
without resubmission exposes a stable explicit state rather than guessing.

Every Matter operation creation and phase transition appends a general durable
`MatterOperationTransitioned` event in the same SQLite transaction. The event
contains only operation ID, operation kind, previous and new phase, and
revision. Its causation actor is derived from the immutable operation binding.
WebSocket replay and live delivery require that exact actor to match the
authenticated subscriber. Targets and sensitive input remain available only
through separately authorized operation reads.

Unlock approval remains a governed common-command operation. The Matter RPC
method delegates to `CommandService::approve_unlock`; it never returns or
accepts an authorization identifier.

## Consequences

- Ordinary diagnostics and operation reads remain safe to log and replay.
- Long-running work survives HTTP disconnects and returns an immediate durable
  handle.
- Sensitive values have a smaller, auditable transport and tracing surface.
- Event cursors can reconnect without leaking another actor's operations.
- The MCP adapter can map tools to the same application services without
  receiving a raw Matter mutation escape hatch.

## Rejected alternatives

- Running controller work synchronously in `/rpc` couples durability to HTTP
  deadlines and cannot return immediately.
- Putting setup and recovery material in ordinary params makes generic tracing,
  retries, and fixtures unsafe.
- Publishing operation targets in general events broadens disclosure beyond the
  actor-filtered operation read boundary.
- Client-supplied actor or policy context defeats durable authentication and
  default-deny authorization.
