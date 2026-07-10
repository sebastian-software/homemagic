# ADR-0001: Use a Rust modular monolith

- Status: Accepted
- Date: 2026-07-11

## Context

Home automation combines latency-sensitive local I/O, long-lived subscriptions,
state projection, automation execution, and several external protocols. Splitting
these concerns into independently deployed services would add operational and
failure complexity before their scaling or release boundaries are known.

At the same time, a single undifferentiated application would make device
adapters, domain rules, and transports difficult to test or replace.

## Decision

HomeMagic starts as one deployable Rust application composed from focused
workspace crates. Crates communicate through typed application ports, not through
network calls. Boundaries must remain compatible with future process extraction,
but no adapter is a separate service until isolation or lifecycle evidence
requires it.

The initial dependency direction is:

1. domain types have no infrastructure dependencies;
2. application services depend on domain types and ports;
3. adapters implement ports;
4. transports invoke application services;
5. the runtime composes all modules.

## Consequences

- Local deployment and debugging remain simple.
- Transactions, startup, and shutdown can initially be coordinated in-process.
- Crate boundaries provide compile-time architecture checks.
- A badly behaved adapter can still affect the process; process isolation remains
  an explicit future option.

