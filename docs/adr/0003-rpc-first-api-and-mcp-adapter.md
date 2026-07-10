# ADR-0003: Make versioned RPC the primary API

- Status: Accepted
- Date: 2026-07-11

## Context

HomeMagic is intended to be controlled by agents and programmatic clients before
it has a comprehensive UI. Home operations naturally contain commands, queries,
subscriptions, deadlines, and structured failures. Treating every operation as a
CRUD resource would obscure these semantics.

MCP is useful for agent discovery and safe tool invocation, but it is an
agent-facing protocol rather than the system's internal domain boundary.

## Decision

The public application contract is a versioned RPC model with:

- unary queries and commands;
- server streams for observations, events, and operation progress;
- explicit request IDs, deadlines, idempotency keys, actor identity, and
  causation/correlation IDs;
- machine-readable error codes and schemas.

The prototype uses JSON-RPC 2.0 over HTTP to make the contract inspectable and
easy to call. The transport is provisional; contract semantics are independent
of JSON-RPC and may later gain a Protobuf/gRPC transport after browser and client
requirements are measured.

MCP is an adapter over application use cases. It exposes curated tools and
resources, applies the same authorization policy, and never bypasses command
validation or the audit trail.

## Consequences

- CLI, agents, UIs, and tests share one application contract.
- The MCP surface can remain smaller and safer than the administrative API.
- Streaming transport and schema versioning require early discipline.
- The prototype avoids prematurely committing to a code-generation ecosystem.

