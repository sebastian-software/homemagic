# ADR-0016: Keep JSON-RPC as the command transport

- Status: Accepted
- Date: 2026-07-11

## Context

EPIC-002 could introduce Protobuf/gRPC, but EPIC-001 already has versioned
JSON-RPC request/response and ordered WebSocket events. Adding a second binary
transport now would duplicate authentication, errors, and streaming semantics
before the application command contract stabilizes.

## Decision

EPIC-002 keeps JSON-RPC 2.0 for command validation, execution, reads,
cancellation, and audit queries. Durable command events use the existing
cursor-based WebSocket stream. Typed application/domain structures—not JSON
objects or method names—remain the authoritative contract.

No public vendor RPC passthrough is added. A future transport ADR may add
Protobuf/gRPC after command schemas and compatibility requirements stabilize; it
must call the same application services and preserve actor, policy, idempotency,
error, and audit semantics.

## Consequences

- One transport surface is secured and tested end to end.
- MCP can map tools to stable application methods without depending on Shelly.
- Binary transport optimization is deferred without closing the option.
