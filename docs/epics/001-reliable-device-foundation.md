# EPIC-001: Reliable Device Foundation

- Milestone: M1
- Status: Ready
- Depends on: M0 discovery prototype
- Unlocks: EPIC-002, Matter feasibility work in EPIC-004

## Objective

Turn the read-only Shelly discovery prototype into a durable device runtime that
survives restarts, continuously reconciles state, handles credentials, and makes
availability and failures explicit.

## User outcome

After starting HomeMagic, previously enrolled Shelly devices appear immediately,
then converge with the local network. Their names, endpoints, capabilities,
availability, and observations remain stable across restarts. State changes arrive
without a manual refresh, and connection problems are diagnosable without reading
debug logs.

## Scope

- embedded SQLite persistence and schema migrations;
- durable device, endpoint, capability, integration, and credential references;
- discovery reconciliation instead of one-scan replacement;
- Shelly Gen2+ authenticated RPC;
- persistent Shelly WebSocket status and event subscriptions;
- reconnect, bounded backoff, refresh fallback, and graceful shutdown;
- availability, freshness, and stale-device lifecycle;
- diagnostics, repair records, and hardware compatibility evidence;
- stable read APIs over the existing JSON-RPC transport.

## Non-goals

- state-changing device commands;
- general automation execution;
- Matter commissioning;
- public plugin ABI;
- cloud relay or remote account management;
- long-term analytical time-series storage.

## Required decisions

- [ ] E1.D1: Add an ADR for SQLite schema ownership, migration policy, and backup
  compatibility.
- [ ] E1.D2: Add an ADR for secret storage on macOS and Linux, including the
  fallback used in headless environments.
- [ ] E1.D3: Record event retention and current-state snapshot policy.

## Workstream E1.1: Persistence foundation

- [ ] Create a `homemagic-storage` crate behind application repository traits.
- [ ] Configure SQLite WAL mode, foreign keys, busy timeout, and explicit
  migrations.
- [ ] Persist installations, integrations, devices, endpoints, aliases, spaces,
  capability descriptors, and latest observations.
- [ ] Keep adapter-native identifiers unique within their integration namespace.
- [ ] Persist schema and capability versions independently from display metadata.
- [ ] Add transactional upsert and reconciliation operations.
- [ ] Add a migration test that upgrades every historical schema fixture.
- [ ] Add backup/restore validation for the current schema.
- [ ] Expose database health and migration version through `system.health`.

## Workstream E1.2: Device and observation lifecycle

- [ ] Replace in-memory-only startup with load-then-reconcile behavior.
- [ ] Model discovery candidate, enrolled, online, degraded, offline, stale, and
  removed lifecycle states.
- [ ] Add `first_seen`, `last_seen`, `last_success`, and freshness timestamps.
- [ ] Preserve a known device when a bounded discovery window misses it.
- [ ] Mark observations stale without rewriting them as an assumed current state.
- [ ] Detect native identity collisions and surface a repair record.
- [ ] Define explicit removal and rediscovery behavior.
- [ ] Publish typed lifecycle and observation events with causation metadata.

## Workstream E1.3: Shelly sessions and authentication

- [ ] Store credential references without storing plaintext secrets in device
  snapshots, logs, or automation data.
- [ ] Implement Shelly digest authentication and reauthentication failure states.
- [ ] Establish one managed WebSocket RPC session per active device.
- [ ] Consume `NotifyStatus` and `NotifyEvent` frames.
- [ ] Merge partial status notifications without dropping unchanged fields.
- [ ] Reconnect with jittered exponential backoff and a configured upper bound.
- [ ] Fall back to a bounded HTTP refresh after subscription gaps.
- [ ] Respect sleeping-device behavior without treating normal sleep as failure.
- [ ] Shut down sessions and mDNS workers cleanly on process termination.
- [ ] Redact credentials, nonces, and sensitive headers from diagnostics.

## Workstream E1.4: Reconciliation and scheduling

- [ ] Run discovery on startup and a configurable periodic schedule.
- [ ] Deduplicate dedicated Shelly and generic HTTP advertisements.
- [ ] Bound concurrent DNS resolution and device requests.
- [ ] Apply per-device timeouts and global refresh deadlines.
- [ ] Ensure one slow device cannot block registry convergence.
- [ ] Coalesce duplicate observations before persistence and fan-out.
- [ ] Record refresh summaries and per-device failure reasons.

## Workstream E1.5: Read API and operations

- [ ] Extend `devices.list` with lifecycle, availability, and freshness filters.
- [ ] Extend `devices.get` with connection and diagnostic summaries.
- [ ] Add `events.subscribe` or an equivalent server-streaming prototype for
  observations and lifecycle events.
- [ ] Add RPC methods for naming devices and assigning spaces without changing
  identity.
- [ ] Add structured repair records and a read API for them.
- [ ] Document database location, backup, credential setup, and recovery.
- [ ] Add a repeatable hardware smoke-test command that emits a redacted report.

## Test and verification checklist

- [ ] Unit tests cover lifecycle transitions and freshness calculations.
- [ ] Repository contract tests run against an isolated temporary database.
- [ ] Migration tests start from every committed schema fixture.
- [ ] Recorded Shelly fixtures cover full status, partial notifications, events,
  authentication challenges, malformed frames, and firmware variations.
- [ ] Reconnect tests use deterministic time and verify backoff bounds.
- [ ] Restart test proves stable device and endpoint IDs.
- [ ] Network-loss test proves stale/offline behavior and recovery.
- [ ] macOS Apple Silicon hardware test covers at least one switch, dimmer, and
  cover.
- [ ] Linux x86_64 CI runs format, Clippy, unit tests, integration tests, and
  migrations.

## Acceptance criteria

- [ ] AC1: A device visible before restart is returned immediately after restart
  with identical HomeMagic, endpoint, and capability identities.
- [ ] AC2: A physical Shelly state change reaches the registry and event stream
  without calling `devices.refresh`.
- [ ] AC3: An authenticated Shelly can be enrolled and observed without exposing
  its password in persisted snapshots, logs, RPC responses, or diagnostics.
- [ ] AC4: Missing discovery advertisements do not delete a known device.
- [ ] AC5: Disconnect, backoff, stale state, reconnect, and recovery are visible
  through structured API data.
- [ ] AC6: Schema migration and backup/restore tests pass from a clean checkout.
- [ ] AC7: Hardware compatibility evidence lists tested model, firmware, host,
  capabilities, and result.

## Exit gate

- [ ] All acceptance criteria contain linked evidence.
- [ ] Required ADRs are accepted and indexed.
- [ ] Operator and API documentation match the shipped behavior.
- [ ] No plaintext secret appears in repository fixtures or captured diagnostics.
- [ ] The full workspace quality gate passes on macOS ARM and Linux x86_64.
- [ ] EPIC-002 is updated with the finalized repository, event, and credential
  contracts.

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| mDNS results vary by host and interface | Persist identity, reconcile, and support explicit diagnostics |
| Notification loss produces stale state | Track freshness and perform bounded refresh fallback |
| Secret storage differs by platform | Isolate a secret-store port and document headless fallback |
| Database design hardens too early | Keep repositories narrow and version all persisted schemas |

## Progress log

- 2026-07-11: Epic created from the verified M0 prototype.
