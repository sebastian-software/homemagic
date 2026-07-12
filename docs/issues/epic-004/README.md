---
id: EPIC-004-ISSUES
epic: EPIC-004
title: Matter Controller Integration issue index
status: in_progress
priority: critical
depends_on: [EPIC-001, EPIC-002]
adrs: [ADR-0002, ADR-0003, ADR-0005, ADR-0008, ADR-0014, ADR-0015]
created: 2026-07-12
updated: 2026-07-12
---

# EPIC-004 Issue Index

Design: [Matter Controller Simulation-First Design](../../superpowers/specs/2026-07-12-matter-controller-simulation-first-design.md)

Plan: [EPIC-004 Matter Controller Implementation Plan](../../plans/2026-07-12-epic-004-matter-controller.md)

| Issue | Status | Depends on | Outcome |
| --- | --- | --- | --- |
| [E4-001](E4-001-matter-decisions.md) | Done | EPIC-001, EPIC-002 | Accepted controller, projection, security, secret, and transport boundaries |
| [E4-002](E4-002-matter-domain-port.md) | Ready | E4-001 | SDK-neutral Matter domain and controller port |
| [E4-003](E4-003-matter-storage.md) | Planned | E4-002 | Durable metadata, operations, authorization, and repair state |
| [E4-004](E4-004-deterministic-controller-simulator.md) | Planned | E4-002 | Deterministic Rust light/lock simulator and contract suite |
| [E4-005](E4-005-capability-projection.md) | Planned | E4-003, E4-004 | Stable projection, reports, subscriptions, and gap recovery |
| [E4-006](E4-006-governed-matter-commands.md) | Planned | E4-003, E4-004, E4-005 | Shared convergence and interactive unlock authorization |
| [E4-007](E4-007-matter-rpc-workflows.md) | Planned | E4-003, E4-005, E4-006 | Simulator-backed durable workflows and authenticated RPC |
| [E4-008](E4-008-controller-feasibility.md) | Planned | E4-004 | Reproducible candidate evidence and accepted selection ADR |
| [E4-009](E4-009-production-controller-adapter.md) | Planned | E4-005, E4-006, E4-008 | Selected production controller adapter |
| [E4-010](E4-010-portability-interoperability.md) | Planned | E4-007, E4-009 | Protected fabric portability and reference interoperability |
| [E4-011](E4-011-matter-exit-audit.md) | Planned | E4-010 | Operations, compatibility, platform, hardware, and exit evidence |

## Evidence classes

| Class | Proves | Does not prove |
| --- | --- | --- |
| Deterministic simulator | HomeMagic application semantics and controller contract | Matter protocol or physical-device compatibility |
| Candidate contract | Adapter compliance with HomeMagic's port | Interoperability with independent implementations |
| External reference | Reproducible protocol interoperability | Compatibility with a named physical product |
| Physical hardware | Exact recorded device/firmware/host behavior | Unrecorded devices, firmware, transports, or certification |

## Progress log

- 2026-07-12: User-approved simulation-first design committed as `9bc214c`.
- 2026-07-12: Dependency-ordered issue set created. E4-001 is ready; physical
  Nuki validation remains an explicit later authorization gate.
- 2026-07-12: E4-001 accepted ADR-0033 through ADR-0038 for the controller port,
  capability projection, unlock authorization, state convergence, fabric
  portability, transport scope, evidence classes, and fixed candidate scorecard.
  E4-002 is ready; no SDK has been selected.
