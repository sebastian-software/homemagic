# EPIC-002: Safe Command Control Plane

- Milestone: M2
- Status: In progress
- Depends on: EPIC-001
- Unlocks: EPIC-003 and full command support in EPIC-004

## Objective

Introduce one typed, authorized, idempotent, and auditable path for every physical
mutation, then use it to control Shelly switches, dimmers, and covers safely.

## User outcome

A client can ask HomeMagic to validate and execute a command, receive a durable
outcome, retry safely, and understand who or what caused the change. Mechanical
and security-sensitive capabilities are governed more strictly than ordinary
comfort actions.

## Scope

- versioned command envelopes and capability-specific command payloads;
- validation against current capability descriptors and constraints;
- actor identity, authorization, policy, causation, and correlation;
- idempotency, deadlines, cancellation, and durable command outcomes;
- append-only audit records;
- API authentication suitable for local CLI, agents, and future UIs;
- Shelly switch, dimmer, and cover command dispatch;
- read-after-write confirmation and mismatch handling;
- dry-run support and operational diagnostics.

## Finalized EPIC-001 contracts

EPIC-002 builds on these accepted boundaries and must extend them without a
parallel command bypass:

- `FoundationRepository` owns atomic current-state, repair, and immutable-event
  commits plus backend-neutral health and cursor reads;
- durable events receive SQLite cursors before `DomainEventSink` fan-out;
- `BroadcastDomainEventSink` is a bounded wake-up mechanism, while cursor-ordered
  storage remains the source of truth for replay and recovery;
- `SecretStore`, zeroizing `SecretValue`, and opaque `SecretRef` are the only
  credential boundary; command records may contain references but never secrets;
- `DeviceId`, `EndpointId`, and versioned `CapabilityDescriptor` address command
  targets independently from names, aliases, spaces, or adapter-native payloads;
- JSON-RPC application methods and future MCP tools call the same application
  services; adapter methods are not public transport APIs.

Command/audit persistence will use forward-only SQLite migrations and its own
retention policy without weakening the 30-day/250,000-row device event contract
from ADR-0009.

## Non-goals

- automation triggers and scheduling;
- natural-language intent resolution;
- lock credential management;
- remote cloud access;
- arbitrary vendor RPC passthrough;
- transactional guarantees across unrelated physical devices.

## Required decisions

- [x] E2.D1: Add an ADR for API authentication and actor identity. Evidence:
  [ADR-0013](../adr/0013-authenticate-rpc-clients-as-durable-actors.md).
- [x] E2.D2: Add an ADR for command durability, idempotency lifetime, and outcome
  retention.
  Evidence: [ADR-0014](../adr/0014-persist-idempotent-command-lifecycles.md).
- [x] E2.D3: Add an ADR for policy evaluation and risk-class defaults. Evidence:
  [ADR-0015](../adr/0015-evaluate-default-deny-risk-policy.md).
- [x] E2.D4: Decide whether the stable binary transport remains JSON-RPC or gains
  a Protobuf/gRPC transport while preserving application semantics.
  Evidence: [ADR-0016](../adr/0016-keep-json-rpc-as-command-transport.md).

## Workstream E2.1: Command domain model

- [x] Define a versioned `CommandEnvelope` with command ID, actor, target,
  capability schema, payload, deadline, idempotency key, correlation ID, and
  causation ID.
- [x] Define states for received, validated, rejected, dispatched, acknowledged,
  confirmed, failed, timed out, and cancelled.
- [x] Define machine-readable validation and execution error codes.
- [x] Separate requested state, device acknowledgement, and observed confirmation.
- [x] Encode capability constraints without vendor-specific command dictionaries.
- [x] Add optimistic concurrency support where stale state makes a command unsafe.

## Workstream E2.2: Policy and authorization

- [ ] Authenticate local RPC clients and create durable actor identities.
- [ ] Authorize by action, capability, target, space, and risk class.
- [ ] Default comfort commands to allow only for authorized local actors.
- [ ] Require explicit mechanical safety policy for position commands.
- [ ] Reserve security-sensitive defaults for explicit approval.
- [ ] Make policy denial explainable without leaking secrets.
- [ ] Apply policy identically to RPC, future MCP, automation, and internal calls.
- [ ] Add rate and concurrency limits per actor and device.

## Workstream E2.3: Durable dispatch and audit

- [ ] Persist the command before physical dispatch.
- [ ] Return the existing outcome for a repeated idempotency key and equivalent
  payload.
- [ ] Reject an idempotency-key collision with a different payload.
- [ ] Enforce deadlines before and during dispatch.
- [ ] Record every transition in an append-only audit trail.
- [ ] Include actor, policy decision, target, adapter result, and causation chain.
- [ ] Redact credentials and sensitive payload fields.
- [ ] Recover safely from process termination between dispatch and confirmation.

## Workstream E2.4: Shelly command adapters

- [ ] Map `on_off.v1` set/toggle to the correct Shelly component method.
- [ ] Map `level.v1` set to dimmer/light constraints and transitions.
- [ ] Map `position.v1` open, close, stop, and go-to-position.
- [ ] Reject go-to-position when calibration or position control is unavailable.
- [ ] Add command origin tags where supported.
- [ ] Confirm outcomes from push observations with bounded read fallback.
- [ ] Surface protection, obstruction, overtemperature, and vendor RPC failures as
  structured outcomes.
- [ ] Prevent duplicate physical dispatch during retries and reconnects.

## Workstream E2.5: RPC and operator surface

- [ ] Add `commands.validate`.
- [ ] Add `commands.execute`.
- [ ] Add `commands.get`.
- [ ] Add `commands.cancel` for cancellable pending work.
- [ ] Add command/audit query filters by actor, target, status, and correlation.
- [ ] Support dry-run validation without dispatch.
- [ ] Provide CLI examples that do not require constructing internal IDs manually
  after selecting a device by query.
- [ ] Document safe rollback and emergency stop behavior.

## Test and verification checklist

- [ ] Property tests cover idempotency and state-machine invariants.
- [ ] Policy matrix tests cover actors, targets, risk classes, and default denial.
- [ ] Adapter fixtures cover success, timeout, reconnect, protection errors, and
  inconsistent observations.
- [ ] Process-restart test covers commands left in every non-terminal state.
- [ ] Audit tests prove immutability, ordering, causation, and redaction.
- [ ] Hardware tests cover switch on/off, dimmer level, cover open/close/stop, and
  calibrated positioning.
- [ ] Hardware tests restore every device to its original state.
- [ ] A physical emergency-stop path is documented for moving devices.

## Acceptance criteria

- [ ] AC1: Retrying an accepted command with the same idempotency key never causes
  a second physical action.
- [ ] AC2: Every mutating request has an authenticated actor and persisted policy
  decision before dispatch.
- [ ] AC3: A caller can distinguish rejection, adapter acknowledgement, observed
  confirmation, timeout, and failure.
- [ ] AC4: Switches, dimmers, and covers can be controlled through common
  capability commands rather than Shelly-specific RPC payloads.
- [ ] AC5: Unauthorized and mechanically unsafe requests are rejected before
  adapter dispatch.
- [ ] AC6: Command and audit history survives restart and contains a complete
  causation chain.
- [ ] AC7: RPC, CLI, and internal application calls exercise the same validation,
  authorization, and dispatch path.

## Exit gate

- [ ] All acceptance criteria contain linked evidence.
- [ ] Required ADRs are accepted and indexed.
- [ ] No adapter exposes a public raw-command bypass.
- [ ] Hardware tests include safe cleanup and produce a redacted report.
- [ ] Threat model and operator documentation cover the shipped control surface.
- [ ] EPIC-003 and EPIC-004 reference the finalized command and policy contracts.

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Network retries duplicate physical actions | Durable idempotency and observation-based confirmation |
| Reported and physical state diverge | Separate acknowledgement from observed confirmation |
| Mechanical movement causes harm | Risk policy, preconditions, deadlines, and stop support |
| Local API becomes an unaudited backdoor | Authenticate all mutating transports and forbid bypasses |

## Progress log

- 2026-07-11: Epic created; blocked on EPIC-001 contracts.
- 2026-07-11: Repository, cursor-event, credential, identity, and transport
  contracts finalized by EPIC-001 and recorded above.
- 2026-07-11: Accepted command authentication, durability, policy, and transport
  ADRs and created the dependency-ordered EPIC-002 issue set.
- 2026-07-11: Completed E2-002 typed command, lifecycle, idempotency,
  precondition, acknowledgement/confirmation, and policy domain contracts.
