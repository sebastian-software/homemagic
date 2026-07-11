# Epic 1 Reliable Device Foundation Design

## Status

- Epic: EPIC-001
- State: Approved by the implementation request
- Date: 2026-07-11

## Purpose

EPIC-001 turns the read-only Shelly prototype into a durable local device
runtime. The design preserves the existing modular-monolith boundary: domain
types describe identity and lifecycle, application ports describe required
behavior, infrastructure crates implement those ports, and transports expose
application services without owning device state.

## Delivery strategy

Work is split into dependency-ordered issues rather than mirroring the epic's
large workstreams. Decisions and domain contracts land before persistence;
persistence lands before reconciliation; authentication lands before managed
sessions; operational APIs and evidence close the milestone.

| Order | Issue | Outcome |
| --- | --- | --- |
| 1 | E1-001 | Persistence, secret-storage, and event-retention decisions |
| 2 | E1-002 | Lifecycle, availability, observation, and event contracts |
| 3 | E1-003 | SQLite-backed repositories and migration tooling |
| 4 | E1-004 | Load-first startup and durable discovery reconciliation |
| 5 | E1-005 | Secret references and Shelly digest authentication |
| 6 | E1-006 | Managed WebSocket subscriptions and partial-state merging |
| 7 | E1-007 | Scheduling, concurrency bounds, recovery, and shutdown |
| 8 | E1-008 | Stable read APIs, metadata mutation, streaming, and repairs |
| 9 | E1-009 | Recovery docs, hardware reports, CI, and exit-gate audit |

## Architecture

`homemagic-domain` remains infrastructure-free. It owns stable identifiers,
lifecycle states, timestamps, observations, diagnostics, repair records, and
typed events. `homemagic-application` owns repository, secret-store, event-sink,
clock, and integration-session ports plus orchestration services.

`homemagic-storage` implements repository ports using SQLite. Migrations are
embedded in the binary and applied before repositories are made available. A
single write transaction atomically reconciles devices, endpoints,
capabilities, current observations, aliases, and spaces. Event history is
bounded independently from current-state snapshots.

`homemagic-shelly` remains the owner of Shelly protocol details. Discovery
produces candidates, authenticated enrollment resolves a durable identity, and
one managed session per active device publishes normalized observations and
events. It never writes SQLite directly.

The executable composes storage, secret storage, adapters, scheduling, and the
JSON-RPC transport. Startup opens and migrates storage, loads known state, starts
the API, and then reconciles the network. Shutdown stops discovery and sessions,
drains bounded work, and closes storage.

## State and data flow

1. Load persisted devices and latest observations into the registry.
2. Publish the loaded snapshot so reads are immediately available.
3. Discover candidates on a bounded startup scan.
4. Match candidates by integration namespace and native identifier.
5. Enroll new devices or update mutable metadata for known devices.
6. Persist reconciliation atomically and publish typed lifecycle events.
7. Start or refresh a managed session for each active device.
8. Merge full or partial observations, persist the current snapshot, and fan out
   deduplicated events.
9. Mark freshness and availability independently when updates stop; never
   rewrite the last observation as an assumed value.

## Identity and lifecycle

HomeMagic IDs remain stable across endpoint, name, address, and restart changes.
Native IDs are unique only inside an integration instance. A discovery miss
never deletes a known device. Explicit removal ends active sessions and marks
the device removed; rediscovery creates a lifecycle transition while retaining
the same stable identity unless the native identity is proven to represent a
different physical device.

Lifecycle and availability are separate. Lifecycle expresses enrollment state
(`candidate`, `enrolled`, `stale`, `removed`); availability expresses runtime
reachability (`online`, `degraded`, `offline`, `sleeping`, `unknown`).

## Failure handling

Infrastructure failures use typed errors with redacted public diagnostics.
Per-device failures do not abort an integration-wide reconciliation. Requests
and DNS resolution are concurrency-bounded and deadline-aware. Session recovery
uses deterministic jittered exponential backoff with an upper bound. A detected
subscription gap triggers a bounded HTTP refresh. Identity collisions create a
repair record and prevent destructive merging.

## Security

Device records contain opaque credential references only. Platform secret
stores are accessed through an application port; the accepted ADR defines the
macOS and Linux implementations and a headless fallback. Passwords, digest
nonces, authorization headers, and derived response material are redacted from
logs, events, diagnostics, fixtures, and RPC payloads.

## API

Existing method names remain compatible. `devices.list` gains lifecycle,
availability, and freshness filters. `devices.get` gains connection and
diagnostic summaries. Metadata mutations change names, aliases, or space
assignments without changing identity. Repair records are readable. Event
delivery starts with a bounded server-streaming transport while preserving typed
application subscriptions for future transports.

## Verification

Each issue carries its own acceptance and verification checklist. The epic gate
also requires repository contract tests, schema-upgrade fixtures,
backup/restore, deterministic lifecycle and backoff tests, restart identity
tests, recorded Shelly protocol fixtures, credential leak scans, hardware smoke
reports, and the full Rust quality gate on macOS ARM and Linux x86_64.

## Deliberate boundaries

This design does not add commands, automation execution, Matter commissioning,
a public plugin ABI, cloud relay, or analytical time-series storage. Those
remain owned by later epics.
