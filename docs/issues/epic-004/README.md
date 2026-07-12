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
| [E4-002](E4-002-matter-domain-port.md) | Done | E4-001 | SDK-neutral Matter domain and controller port |
| [E4-003](E4-003-matter-storage.md) | Done | E4-002 | Durable metadata, operations, authorization, and repair state |
| [E4-004](E4-004-deterministic-controller-simulator.md) | Done | E4-002 | Deterministic Rust light/lock simulator and cross-platform contract evidence |
| [E4-005](E4-005-capability-projection.md) | Done | E4-003, E4-004 | Stable projection, reports, subscriptions, and gap recovery |
| [E4-006](E4-006-governed-matter-commands.md) | Done | E4-003, E4-004, E4-005 | Shared convergence and interactive unlock authorization |
| [E4-006-01](E4-006-01-access-control-command-contract.md) | Done | E4-005 | Typed access-control command and approval authority contracts |
| [E4-006-02](E4-006-02-desired-state-supersession.md) | Done | E4-006-01 | Monotonic desired slots and pre-dispatch supersession |
| [E4-006-03](E4-006-03-matter-command-adapters.md) | Done | E4-006-01, E4-006-02 | Governed controller dispatch and observation confirmation |
| [E4-006-04](E4-006-04-interactive-unlock-authorization.md) | Done | E4-006-01, E4-006-02, E4-006-03 | Exact interactive single-use unlock admission |
| [E4-007](E4-007-matter-rpc-workflows.md) | In progress | E4-003, E4-005, E4-006 | Simulator-backed durable workflows and authenticated RPC |
| [E4-007-01](E4-007-01-administration-service.md) | Done | E4-003, E4-005, E4-006 | Authenticated durable administration boundary |
| [E4-007-02](E4-007-02-fabric-workflows.md) | Done | E4-007-01 | Fabric status, creation, and simulator portability workflows |
| [E4-007-02-01](E4-007-02-01-fabric-status-create.md) | Done | E4-007-01 | Idempotent staged fabric creation and status |
| [E4-007-02-02](E4-007-02-02-simulator-export.md) | Done | E4-007-02-01 | Explicit sensitive simulator export |
| [E4-007-02-03](E4-007-02-03-simulator-restore-boundary.md) | Done | E4-007-02-01, E4-007-02-02 | Simulator restore and production format guard |
| [E4-007-03](E4-007-03-node-operation-workflows.md) | Done | E4-007-01, E4-007-02 | Commissioning, removal, cancellation, and recovery |
| [E4-007-03-01](E4-007-03-01-commissioning-target-admission.md) | Done | E4-007-02 | Fabric-scoped commissioning admission and sensitive input boundary |
| [E4-007-03-02](E4-007-03-02-commissioning-projection.md) | Done | E4-007-03-01 | Atomic commissioned-node projection commit |
| [E4-007-03-03](E4-007-03-03-cancellation-recovery.md) | Done | E4-007-03-01, E4-007-03-02 | Cancellation and restart reconciliation |
| [E4-007-03-04](E4-007-03-04-node-inventory.md) | Done | E4-007-03-02 | Authenticated bounded node inventory |
| [E4-007-03-05](E4-007-03-05-node-removal.md) | Done | E4-007-03-03, E4-007-03-04 | Removal with visible partial cleanup |
| [E4-007-04](E4-007-04-subscription-diagnostics-repair.md) | In progress | E4-007-01, E4-007-03 | Bounded diagnostics and subscription repair |
| [E4-007-04-01](E4-007-04-01-read-only-diagnostics.md) | Done | E4-007-03 | Authenticated bounded read-only diagnostics |
| [E4-007-04-02](E4-007-04-02-subscription-status.md) | Ready | E4-007-04-01 | Deterministic subscription status |
| [E4-007-04-03](E4-007-04-03-explicit-subscription-repair.md) | Planned | E4-007-04-02 | Explicit gap-read and resubscribe repair |
| [E4-007-04-04](E4-007-04-04-repair-restart-exhaustion.md) | Planned | E4-007-04-03 | Restart and exhaustion reconciliation |
| [E4-007-05](E4-007-05-authenticated-rpc-events.md) | Planned | E4-007-02, E4-007-03, E4-007-04 | Authenticated RPC schemas and durable operation events |
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
- 2026-07-12: E4-002 completed validated SDK-neutral domain contracts, the
  object-safe async controller port, sensitive-value redaction, persisted
  round trips, architecture documentation, and an executable dependency guard.
  E4-003 and E4-004 are ready and may proceed independently.
- 2026-07-12: E4-003 completed schema 6 and the application-owned durable Matter
  repository with optimistic revisions, atomic operation/repair and command
  convergence facts, single-use unlock authorization, restart recovery,
  protected retention, migration fixtures, and secret-safe backup evidence.
  E4-004 remains ready.
- 2026-07-12: E4-004 implemented the pure-Rust deterministic controller,
  versioned light/lock fixtures, dispatch barriers, fault and restart scripts,
  typed simulator-export isolation, property tests, and committed normalized
  trace. Local macOS ARM64 evidence is green; the committed Linux x86_64 CI hash
  job still needs an actual run before the issue is done.
- 2026-07-12: Public CI run `29196515664` passed the committed trace hash on
  macOS ARM64 and Linux x86_64 and passed the complete Linux quality,
  migration, and secret-scan job. E4-004 is done and E4-005 is ready.
- 2026-07-12: E4-005 implemented versioned common-capability projection for the
  simulator light and lock, report ordering and causation, descriptor
  invalidation, stable restart identities, bounded diagnostics, and deterministic
  subscription gap recovery. Local CI-equivalent gates pass; public CI is
  pending before closure.
- 2026-07-12: Public CI run `29197306255` passed the full Linux x86_64 quality
  job and simulator hashes on Linux x86_64 and macOS ARM64. E4-005 is done and
  E4-006 is ready.
- 2026-07-12: E4-006 completed typed Matter command mapping, desired-state
  supersession, bounded observation confirmation, and exact interactive unlock
  approval with atomic at-most-once dispatch admission. E4-007 is ready.
- 2026-07-12: Public CI run `29199088173` verified E4-006 with the complete
  Linux x86_64 quality job and deterministic hashes on Linux x86_64 and macOS
  ARM64.
- 2026-07-12: E4-007-01 added schema 8 actor-bound operation admission, exact
  administration grants, canonical idempotency, bounded owner reads, safe
  pre-controller cancellation, and structured failure/repair persistence.
  E4-007-02 is ready.
- 2026-07-12: Public CI run `29199747179` verified E4-007-01 with Linux x86_64
  quality and deterministic simulator hashes on Linux x86_64 and macOS ARM64.
- 2026-07-12: E4-007-02 implemented restart-safe fabric creation, explicit
  simulator export/restore, sensitive-value isolation, and production-format
  rejection. Targeted contracts, exact CI-format Clippy, boundary/secret scans,
  and the full privileged workspace test suite pass; commit, push, and public CI
  remain pending.
- 2026-07-12: Public CI run `29202622965` passed Linux x86_64 quality and
  deterministic simulator hashes on Linux x86_64 and macOS ARM64. E4-007-02 is
  done and E4-007-03 is ready.
- 2026-07-12: E4-007-03 was decomposed into five implementation slices. ADR-0040
  makes commissioning fabric-scoped until the controller returns an
  authoritative node ID; E4-007-03-01 is ready.
- 2026-07-12: E4-007-03-01 implemented schema 10 operation-to-node identity,
  fabric-scoped admission, and the redacted setup-input boundary. All local
  gates pass; commit, push, and public CI remain pending.
- 2026-07-12: Public CI run `29203093982` passed Linux x86_64 quality and
  simulator hashes on Linux x86_64 and macOS ARM64. E4-007-03-01 is done and
  E4-007-03-02 is ready.
- 2026-07-12: E4-007-03-02 implemented exact phase reconciliation, bounded
  initial reads, logical subscription, and an atomic commissioned-node
  projection commit. All local gates pass; commit, push, and public CI remain
  pending.
- 2026-07-12: Public CI run `29203595736` passed Linux x86_64 quality and
  simulator hashes on Linux x86_64 and macOS ARM64. E4-007-03-02 is done and
  E4-007-03-03 is ready.
- 2026-07-12: E4-007-03-03 implements owner-isolated local and in-flight
  cancellation, atomic dual-operation reconciliation, and fail-closed bounded
  restart recovery. Local CI-equivalent gates pass; public CI is pending.
- 2026-07-12: Public CI run `29204270373` passed Linux x86_64 quality and
  simulator hashes on Linux x86_64 and macOS ARM64. E4-007-03-03 is done and
  E4-007-03-04 is ready.
- 2026-07-12: E4-007-03-04 authenticated bounded durable node inventory is
  implemented and all local CI-equivalent gates pass. Public CI remains pending.
- 2026-07-12: Public CI run `29204953299` passed Linux x86_64 quality and
  simulator hashes on Linux x86_64 and macOS ARM64. E4-007-03-04 is done and
  E4-007-03-05 is ready.
- 2026-07-12: E4-007-03-05 node removal and visible partial cleanup are
  implemented and all local CI-equivalent gates pass. Public CI remains pending.
- 2026-07-12: Public CI run `29205464608` passed Linux x86_64 quality and
  simulator hashes on Linux x86_64 and macOS ARM64. E4-007-03 and all five
  children are done; E4-007-04 is ready.
- 2026-07-12: E4-007-04 was decomposed into read-only diagnostics followed by
  explicit bounded subscription repair. E4-007-04-01 is ready.
- 2026-07-12: E4-007-04-01 bounded read-only diagnostics are implemented and
  local CI-equivalent gates pass. Public CI remains pending.
- 2026-07-12: E4-007-04-01 passed public Linux x86_64 and macOS ARM64 CI and is
  done. E4-007-04-02 is ready.
