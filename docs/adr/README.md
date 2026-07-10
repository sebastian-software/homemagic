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

