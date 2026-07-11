# Roadmap

This roadmap is evidence-driven. A milestone is complete only when its acceptance
evidence exists in the repository or a repeatable hardware test report.

## M0: Architecture and executable discovery prototype

Status: implemented on macOS Apple Silicon; Linux x86_64 CI configured.

Goal: prove the modular boundaries and see local Shelly Gen2+ devices through a
HomeMagic-owned API.

Acceptance evidence:

- Home Assistant architecture research and initial ADRs are committed;
- the application builds on macOS Apple Silicon;
- tests and Clippy pass;
- `_shelly._tcp.local.` or Shelly-filtered `_http._tcp.local.` discovery resolves
  device addresses;
- device info and unauthenticated status are projected into the capability model;
- `devices.list` returns the current snapshots through JSON-RPC;
- a fixture-backed test proves projection for switch, light, and cover examples.

## Next delivery milestones

The next five milestones are defined as checkable epics. The
[epic index](epics/README.md) is the source of truth for dependencies, progress,
acceptance evidence, and exit gates.

### M1: Reliable device foundation

See [EPIC-001](epics/001-reliable-device-foundation.md).

Persist device identity and state, reconcile discovery, support authenticated
Shelly RPC, consume push updates, and make lifecycle and repair state explicit.

### M2: Safe command control plane

See [EPIC-002](epics/002-safe-command-control-plane.md).

Create the single authorized, idempotent, and audited mutation path, then control
Shelly switches, dimmers, and covers through common capabilities.

### M3: Agent-authored automation engine

See [EPIC-003](epics/003-agent-authored-automation-engine.md).

Deliver a versioned automation IR, validation, deterministic simulation, governed
activation, durable execution, and explainable traces.

### M4: Matter controller integration

See [EPIC-004](epics/004-matter-controller-integration.md).

Select the controller boundary through evidence, commission Matter-over-Wi-Fi
devices, map clusters to capabilities, subscribe, control, and protect fabric
state. Feasibility work may start during M1; full control depends on M2.

The implementation must preserve the 95% Rust policy. Any non-Rust controller or
FFI exception requires the evidence specified by ADR-0005.

### M5: MCP and intent-driven interaction

See [EPIC-005](epics/005-mcp-intent-driven-interaction.md).

Expose curated tools and resources, resolve household language safely, support
the automation lifecycle, and establish transport-neutral presentation
descriptors for later generated UIs.

## Later horizons

After the five delivery epics:

- security-sensitive lock capability and credential management;
- camera discovery, snapshots, streams, privacy, and media backend ADR;
- first robot-vacuum integration selected from documented local APIs;
- generated, need-oriented web/mobile interaction surfaces;
- accessibility and mobile interaction acceptance criteria.

## Deferred until evidence justifies them

- public third-party plugin ABI;
- integration-per-process deployment;
- Linux ARM support commitment;
- cloud relay or hosted account system;
- broad Home Assistant configuration compatibility;
- arbitrary agent-generated executable code.
