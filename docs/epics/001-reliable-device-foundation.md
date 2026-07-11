# EPIC-001: Reliable Device Foundation

- Milestone: M1
- Status: In progress
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

## Implementation tracking

- [Implementation design](../superpowers/specs/2026-07-11-epic-001-reliable-device-foundation-design.md)
- [Dependency-ordered issue index](../issues/epic-001/README.md)

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

- [x] E1.D1: Add an ADR for SQLite schema ownership, migration policy, and backup
  compatibility. Evidence: [ADR-0007](../adr/0007-sqlite-schema-migrations-and-backups.md).
- [x] E1.D2: Add an ADR for secret storage on macOS and Linux, including the
  fallback used in headless environments. Evidence:
  [ADR-0008](../adr/0008-platform-secret-stores-and-headless-vault.md).
- [x] E1.D3: Record event retention and current-state snapshot policy. Evidence:
  [ADR-0009](../adr/0009-current-state-and-event-retention.md).

## Workstream E1.1: Persistence foundation

- [x] Create a `homemagic-storage` crate behind application repository traits.
  Evidence: `crates/homemagic-storage` and `FoundationRepository`.
- [x] Configure SQLite WAL mode, foreign keys, busy timeout, and explicit
  migrations. Evidence: `open_connection` and migration tests.
- [x] Persist installations, integrations, devices, endpoints, aliases, spaces,
  capability descriptors, and latest observations. Evidence:
  `complete_projection.rs`.
- [x] Keep adapter-native identifiers unique within their integration namespace.
  Evidence: schema constraint and native-identity collision test.
- [x] Persist schema and capability versions independently from display metadata.
  Evidence: migration ledger and normalized `capabilities` table.
- [x] Add transactional upsert and reconciliation operations. Evidence:
  `FoundationRepository::apply` and rollback tests.
- [x] Add a migration test that upgrades every historical schema fixture.
  Evidence: `migration_fixtures.rs`.
- [x] Add backup/restore validation for the current schema. Evidence:
  `backup_restore.rs`.
- [ ] Expose database health and migration version through `system.health`.

## Workstream E1.2: Device and observation lifecycle

- [x] Replace in-memory-only startup with load-then-reconcile behavior. Evidence:
  durable daemon composition and load-first application tests.
- [x] Model discovery candidate, enrolled, online, degraded, offline, stale, and
  removed lifecycle states. Evidence: `crates/homemagic-domain/src/lifecycle.rs`.
- [x] Add `first_seen`, `last_seen`, `last_success`, and freshness timestamps.
  Evidence: `DeviceTimestamps`, `ObservedValue`, and `FreshnessPolicy`.
- [x] Preserve a known device when a bounded discovery window misses it.
  Evidence: `discovery_miss_should_not_change_known_device`.
- [x] Mark observations stale without rewriting them as an assumed current state.
  Evidence: freshness is calculated separately from field-level `ObservedValue`.
- [x] Detect native identity collisions and surface a repair record. Evidence:
  collision reconciliation test and `RepairKind::IdentityCollision`.
- [x] Define explicit removal and rediscovery behavior. Evidence: application
  tombstone and reconciliation tests.
- [ ] Publish typed lifecycle and observation events with causation metadata.
  Partial evidence: correlated lifecycle/availability event fan-out is complete;
  live observation publication remains in E1-006.

## Workstream E1.3: Shelly sessions and authentication

- [x] Store credential references without storing plaintext secrets in device
  snapshots, logs, or automation data. Evidence: `SecretRef`, `SecretStore`,
  zeroizing `SecretValue`, and encrypted/platform adapters.
- [x] Implement Shelly digest authentication and reauthentication failure states.
  Evidence: strict SHA-256 challenge response, bounded stale retry, and stable
  credential repair records.
- [ ] Establish one managed WebSocket RPC session per active device.
- [ ] Consume `NotifyStatus` and `NotifyEvent` frames.
- [ ] Merge partial status notifications without dropping unchanged fields.
- [ ] Reconnect with jittered exponential backoff and a configured upper bound.
- [ ] Fall back to a bounded HTTP refresh after subscription gaps.
- [ ] Respect sleeping-device behavior without treating normal sleep as failure.
- [ ] Shut down sessions and mDNS workers cleanly on process termination.
- [x] Redact credentials, nonces, and sensitive headers from diagnostics.
  Evidence: secret/challenge debug redaction and captured-error canary tests.

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

- [x] Unit tests cover lifecycle transitions and freshness calculations. Evidence:
  `cargo test -p homemagic-domain --all-features --locked`.
- [x] Repository contract tests run against an isolated temporary database.
  Evidence: `crates/homemagic-storage/tests`.
- [x] Migration tests start from every committed schema fixture. Evidence:
  `migration_fixtures.rs`.
- [ ] Recorded Shelly fixtures cover full status, partial notifications, events,
  authentication challenges, malformed frames, and firmware variations.
- [ ] Reconnect tests use deterministic time and verify backoff bounds.
- [x] Restart test proves stable device and endpoint IDs. Evidence: storage
  reopen contract and daemon bootstrap identity reuse test.
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
- [x] AC3: An authenticated Shelly can be enrolled and observed without exposing
  its password in persisted snapshots, logs, RPC responses, or diagnostics.
  Evidence: authenticated transport fixtures, opaque persisted references, and
  credential-canary tests.
- [x] AC4: Missing discovery advertisements do not delete a known device.
  Evidence: `discovery_miss_should_not_change_known_device`.
- [ ] AC5: Disconnect, backoff, stale state, reconnect, and recovery are visible
  through structured API data.
- [ ] AC6: Schema migration and backup/restore tests pass from a clean checkout.
- [ ] AC7: Hardware compatibility evidence lists tested model, firmware, host,
  capabilities, and result.

## Exit gate

- [ ] All acceptance criteria contain linked evidence.
- [ ] Required ADRs are accepted and indexed.
- [ ] Operator and API documentation match the shipped behavior.
- [x] No plaintext secret appears in repository fixtures or captured diagnostics.
  Evidence: sanitized challenge fixtures and captured diagnostic canary tests.
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
- 2026-07-11: Implementation started with a dependency-ordered design and nine
  repository-tracked issues. Evidence: `docs/issues/epic-001/README.md`.
- 2026-07-11: Completed E1-001 and accepted persistence, secret-storage, and
  retention decisions in ADR-0007 through ADR-0009.
- 2026-07-11: Completed E1-002 domain lifecycle contracts and application ports;
  full workspace format, Clippy, tests, and doctests pass.
- 2026-07-11: Completed E1-005 secret-safe Shelly digest authentication,
  platform/headless secret adapters, stable repairs, and redaction tests.
- 2026-07-11: Completed E1-003 SQLite storage, schema migrations, repository
  contracts, backup/restore, and health diagnostics.
- 2026-07-11: Completed E1-004 load-first startup and durable discovery
  reconciliation; local daemon health and device-list smoke tests pass.
