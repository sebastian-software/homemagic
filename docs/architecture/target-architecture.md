# Target Architecture

## Purpose

HomeMagic is a local-first home automation runtime optimized for intent-driven,
programmatic operation. Home Assistant is a compatibility and behavior reference,
not a source-compatible target.

## System shape

The first deployment is a modular monolith with one process and multiple Rust
crates. The architecture follows ports and adapters:

```text
Clients / agents / generated UIs
          |
    RPC and MCP adapters
          |
   Application services
   /       |          \
Registry  Automation  Policy/Audit
   |         Engine       |
   +---- Capability Kernel+
                 |
          Integration ports
        /       |        |       \
     Shelly   Matter   Cameras   Robots
```

The diagram describes dependency direction, not separate services.

## Domain model

### Installation and spaces

An installation is the security and persistence boundary. Spaces form a semantic
graph such as home, floor, room, and outdoor zone. Devices and endpoints may be
associated with spaces without changing their identity.

### Devices and endpoints

A device stores stable adapter identity, manufacturer metadata, connectivity,
availability, and mutable display metadata. An endpoint is an independently
addressable channel or function. For example, a two-channel Shelly has one device
and two switch endpoints; a bridged Matter node may expose several endpoints.

### Capabilities

Capabilities are small typed interfaces. They declare readable observations,
accepted commands, constraints, units, risk classification, and schema version.
An endpoint composes capabilities instead of inheriting a large device class.

The initial vocabulary is deliberately small:

| Capability | Example observations | Example commands |
| --- | --- | --- |
| `availability.v1` | online, last seen | refresh |
| `on_off.v1` | on | set, toggle |
| `level.v1` | percentage | set level |
| `position.v1` | position, motion | open, close, stop, go to |
| `power.v1` | watts, volts, amperes | none |
| `energy.v1` | watt-hours | reset, when supported |
| `diagnostics.v1` | temperature, errors, firmware | identify, update later |

Vendor extensions use a namespaced schema and are never required to understand a
common capability.

## State, commands, and events

- Observations describe what an adapter reported and when.
- Desired state is not silently treated as observed state.
- Commands are validated requests with an actor, target, deadline, idempotency
  key, and causation chain.
- Events are immutable facts, including device events, command outcomes,
  automation transitions, and administrative changes.
- The current-state projection can be rebuilt from persisted device snapshots and
  subsequent events, but the first version is not a fully event-sourced system.

## Integration lifecycle

Each adapter implements discovery, enrollment, refresh/subscription, command
dispatch, diagnostics, and shutdown ports as applicable. Adapters own protocol
details; they do not own automation or presentation behavior.

Discovery produces candidates. Enrollment verifies identity and creates a durable
device record. Runtime sessions publish observations and events. Availability is
explicit and includes reason and last-success timestamps.

## API model

The application contract is RPC-first. Initial method families are:

- `system.*`: health, version, and capabilities;
- `devices.*`: discover, list, get, name, locate, and diagnose;
- `commands.*`: validate, execute, and inspect;
- `events.*`: subscribe and query;
- `automations.*`: draft, validate, simulate, approve, activate, and inspect;
- `policies.*`: inspect and manage activation/command rules.

MCP maps a curated subset onto tools and resources. It resolves natural-language
intent through the same registry and submits the same typed commands and
automation documents as every other client.

## Automation engine

The engine consumes normalized events and observations, evaluates a versioned
declarative document, and emits commands through application services. It does
not communicate with device adapters directly.

The intermediate representation has bounded constructs rather than arbitrary
code: typed triggers, boolean and temporal conditions, action sequences,
parallel/race groups, delays, variables, retry/timeout policy, and explicit
concurrency modes. Every draft is statically validated before simulation or
activation.

## Persistence

The planned default is embedded SQLite with migrations and WAL mode. It stores
configuration, identity, automation versions, policies, audit records, and
bounded event/history data. High-volume camera media remains outside the core
database. A retention policy controls telemetry growth.

SQLite is not required for the first discovery prototype; the in-memory registry
keeps the first slice focused and makes the persistence boundary explicit.

## Security

- Local operation does not imply unauthenticated operation.
- All mutating RPC calls carry an actor and are authorized by capability and
  target.
- Secrets are stored through an operating-system or encrypted-secret adapter,
  never in automation documents or logs.
- Security-sensitive capabilities have conservative default policy.
- MCP receives no privileged bypass.
- Camera streams and lock credentials use separate, narrowly scoped permissions.

## Deployment targets

The first supported targets are macOS Apple Silicon and Linux x86_64. Platform
features such as mDNS, network interfaces, Bluetooth, Thread, and media backends
sit behind ports. Linux ARM is intentionally not part of the first support claim.

## Quality strategy

- unit tests for domain invariants and capability projection;
- contract tests shared by integration adapters;
- recorded protocol fixtures for deterministic integration tests;
- simulated clocks and event streams for automation tests;
- hardware-in-the-loop smoke tests for explicitly listed devices;
- format, Clippy, tests, dependency audit, and documentation checks in CI.

