# Architecture Decision Records

HomeMagic records significant architectural decisions as ADRs. ADRs are
immutable after acceptance except for corrections and status changes. A later
decision supersedes an earlier one rather than silently rewriting history.

## Status values

- `Proposed`: under active review
- `Accepted`: current project direction
- `Superseded`: replaced by another ADR
- `Rejected`: considered and not selected

## Index

- [ADR-0001: Use a Rust modular monolith](0001-rust-modular-monolith.md)
- [ADR-0002: Model devices through composable capabilities](0002-capability-oriented-domain-model.md)
- [ADR-0003: Make versioned RPC the primary API](0003-rpc-first-api-and-mcp-adapter.md)
- [ADR-0004: Store automations as declarative, governed documents](0004-agent-authored-automations.md)
- [ADR-0005: Keep first-party runtime code at least 95% Rust](0005-rust-and-ffi-policy.md)
- [ADR-0006: Use Shelly Gen2+ as the first device vertical slice](0006-shelly-first-vertical-slice.md)
- [ADR-0007: Own SQLite schemas and use forward-only migrations](0007-sqlite-schema-migrations-and-backups.md)
- [ADR-0008: Store device credentials behind platform secret stores](0008-platform-secret-stores-and-headless-vault.md)
- [ADR-0009: Separate current state from bounded event retention](0009-current-state-and-event-retention.md)
- [ADR-0010: Own one managed session per active device](0010-managed-device-sessions-and-notification-gaps.md)
- [ADR-0011: Bound runtime scheduling and recovery loops](0011-bounded-runtime-scheduling-and-recovery.md)
- [ADR-0012: Stream durable events over JSON-RPC WebSockets](0012-json-rpc-websocket-event-subscriptions.md)
- [ADR-0013: Authenticate RPC clients as durable actors](0013-authenticate-rpc-clients-as-durable-actors.md)
- [ADR-0014: Persist idempotent command lifecycles before dispatch](0014-persist-idempotent-command-lifecycles.md)
- [ADR-0015: Evaluate a default-deny risk policy before dispatch](0015-evaluate-default-deny-risk-policy.md)
- [ADR-0016: Keep JSON-RPC as the command transport](0016-keep-json-rpc-as-command-transport.md)
- [ADR-0017: Version automation documents and normalized plans independently](0017-version-automation-documents-and-plans.md)
- [ADR-0018: Use deterministic automation time and never replay missed schedules](0018-use-deterministic-automation-time-and-scheduling.md)
- [ADR-0019: Govern automation activation with capability Safety Profiles](0019-govern-automation-activation-with-safety-profiles.md)
- [ADR-0020: Retain automation versions, runs, and traces independently](0020-retain-automation-evidence-independently.md)
- [ADR-0021: Persist automation group continuations in run state](0021-persist-automation-group-continuations.md)
- [ADR-0022: Persist automation command attempts explicitly](0022-persist-automation-command-attempts.md)
