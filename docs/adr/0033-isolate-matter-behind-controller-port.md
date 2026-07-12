# ADR-0033: Isolate Matter behind an SDK-neutral controller port

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0001, ADR-0002, ADR-0005, EPIC-004, E4-001

## Context

HomeMagic needs production Matter controller behavior, but the available
controller implementations differ in API shape, persistence ownership,
commissioning support, subscriptions, native dependencies, and platform
coverage. Selecting one before the application contract exists would allow SDK
types and assumptions to become permanent domain boundaries.

Application behavior must also be developed without physical hardware while
keeping simulator results distinct from real protocol compatibility.

## Decision

The application defines an async `MatterController` port using only
HomeMagic-owned bounded request, response, identity, event, cursor, operation,
and redacted error types.

The port covers:

- HomeMagic fabric create, load, inspect, export, and restore;
- commissioning start, cancellation, inspection, and recovery;
- node and endpoint inventory;
- descriptors, device types, features, attributes, and events;
- subscriptions and bounded reads;
- adapter-private protocol invocation;
- node removal and incomplete-cleanup reporting.

`homemagic-domain` owns stable value types. `homemagic-application` owns the port
and workflows. `homemagic-matter` implements the port and contains every SDK,
protocol, callback, native-library, and adapter-private invocation type. API and
MCP crates cannot depend on an SDK or construct a raw protocol invocation.

The first implementation is an independent deterministic in-process Rust
simulator. It implements the same port with virtual time, deterministic
identities, scripted state, dispatch barriers, fault injection, subscription
loss, and restart checkpoints. It does not implement or claim to test the Matter
wire protocol.

A reusable controller contract suite runs against the simulator and every later
candidate/production adapter. A candidate's own simulator is insufficient for
selection; independent reference interoperability remains separate evidence.

## Evidence boundary

- deterministic simulator evidence proves HomeMagic application and port
  semantics only;
- candidate contract evidence proves conformance to the HomeMagic port;
- external reference evidence proves the recorded independent protocol
  lifecycle;
- physical evidence proves only the exact recorded device, firmware, transport,
  host, adapter, and actions.

No lower evidence class can complete a higher-class acceptance criterion.

## Consequences

- Application and domain work can proceed before controller selection.
- Replacing an SDK does not change public device, command, event, or RPC models.
- One additional abstraction and simulator must be maintained.
- The port must not grow into a public raw-cluster API.
- Production selection remains blocked on the fixed E4-008 evidence matrix.
