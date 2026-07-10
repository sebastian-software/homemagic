# Roadmap

This roadmap is evidence-driven. A milestone is complete only when its acceptance
evidence exists in the repository or a repeatable hardware test report.

## M0: Architecture and executable discovery prototype

Goal: prove the modular boundaries and see local Shelly Gen2+ devices through a
HomeMagic-owned API.

Acceptance evidence:

- Home Assistant architecture research and initial ADRs are committed;
- the application builds on macOS Apple Silicon;
- tests and Clippy pass;
- `_shelly._tcp.local.` discovery resolves device addresses;
- device info and unauthenticated status are projected into the capability model;
- `devices.list` returns the current snapshots through JSON-RPC;
- a fixture-backed test proves projection for switch, light, and cover examples.

## M1: Reliable Shelly Gen2+ control

- durable SQLite registry and migrations;
- authenticated Shelly RPC;
- WebSocket status/event subscriptions with reconnect and backoff;
- typed commands for switch, dimmer, and cover;
- energy and diagnostic observations;
- idempotency, deadlines, command outcomes, and audit records;
- hardware compatibility matrix and repeatable smoke-test command.

## M2: Agent-authored automation foundation

- versioned automation intermediate representation;
- type/reference validation;
- deterministic engine with simulated time;
- recorded-event simulation and explainable trace;
- risk classification and approval policy;
- RPC lifecycle for draft through activation;
- initial MCP tools and resources using the official Rust SDK.

## M3: Matter controller integration

- decide the Matter controller implementation in a dedicated ADR;
- commission Wi-Fi devices on macOS and Linux where platform support permits;
- import nodes/endpoints/clusters into capabilities;
- subscriptions, commands, fabric backup, and OTA visibility;
- Thread/BLE boundary and supported-host documentation;
- conformance fixtures derived from Matter device types and clusters.

The implementation must preserve the 95% Rust policy. Any non-Rust controller or
FFI exception requires the evidence specified by ADR-0005.

## M4: Security-sensitive and media integrations

- lock capability, credential isolation, and approval policy;
- camera discovery, snapshots, and stream descriptors;
- selective media backend ADR for codec/transport dependencies;
- explicit privacy zones, retention, and access audit;
- first robot-vacuum adapter selected from documented local APIs.

## M5: Generated interaction surfaces

- need-oriented view schema derived from capabilities and context;
- agent-generated views that remain editable and versioned;
- reference web client generated from the same RPC schemas;
- accessibility and mobile interaction acceptance criteria.

## Deferred until evidence justifies them

- public third-party plugin ABI;
- integration-per-process deployment;
- Linux ARM support commitment;
- cloud relay or hosted account system;
- broad Home Assistant configuration compatibility;
- arbitrary agent-generated executable code.

