# EPIC-004: Matter Controller Integration

- Milestone: M4
- Status: Active; simulation-first implementation planning complete
- Depends on: EPIC-001 for identity/lifecycle and EPIC-002 for full command delivery
- Unlocks: Matter device coverage in EPIC-005
- Design: [Matter Controller Simulation-First Design](../superpowers/specs/2026-07-12-matter-controller-simulation-first-design.md)
- Plan: [EPIC-004 Matter Controller Implementation Plan](../plans/2026-07-12-epic-004-matter-controller.md)
- Issues: [EPIC-004 issue index](../issues/epic-004/README.md)

## Objective

Add production-capable Matter controller support while preserving HomeMagic's
capability model, supported platforms, local-first behavior, and 95% Rust policy.

## User outcome

A user can commission supported Matter-over-Wi-Fi devices into a HomeMagic fabric,
see their nodes and endpoints as normal HomeMagic devices and capabilities,
observe changes, control supported functions, inspect diagnostics, and back up the
fabric without learning Matter cluster internals.

## Scope

- time-boxed Rust controller feasibility study and controller ADR;
- controller lifecycle behind an integration port;
- fabric creation, persistence, backup, and restore;
- commissioning and removal for Matter-over-Wi-Fi;
- node, endpoint, device-type, cluster, attribute, event, and command mapping;
- subscriptions, reconnect, availability, and data-version handling;
- capability commands through EPIC-002;
- diagnostics, attestation/certification visibility, and OTA visibility;
- macOS Apple Silicon and Linux x86_64 platform evidence;
- explicit Thread/BLE boundary and follow-up plan.

## Non-goals

- implementing the complete Matter specification from scratch;
- claiming certification before completing the relevant external process;
- exposing raw cluster writes as the primary API;
- Matter bridge/server behavior that exposes HomeMagic devices to other fabrics;
- full Thread commissioning unless the feasibility ADR explicitly accepts it;
- vendor-specific clusters without fixtures and a namespaced extension contract.

## Finalized EPIC-002 contracts

- Matter exposes only normalized capability commands to callers; raw cluster
  writes are private adapter implementation details.
- The Matter adapter implements `CommandDispatcher` and `CommandConfirmation`;
  all validation, actor policy, idempotency, deadlines, audit, and recovery stay
  in the shared `CommandService`.
- Interaction Model acknowledgement is not physical confirmation. Subscribed or
  bounded-read attributes provide the observed outcome.
- Window Covering commands retain mechanical risk, fresh descriptor/feature
  constraints, explicit grants, stop behavior, and physical safety guidance.
- Door Lock commands remain security-risk and require exact capability grants
  plus a dedicated policy and threat-model review before enablement.
- Matter credentials and fabric secrets use `SecretStore` and never enter command
  envelopes, audit records, events, or diagnostics.

## Required decisions

- [ ] E4.D1: Benchmark credible Rust-native controller libraries against required
  controller, commissioning, subscription, persistence, and platform features.
- [ ] E4.D2: Accept an ADR selecting native Rust, narrowly scoped FFI, or an
  isolated sidecar, including the ADR-0005 evidence for any exception.
- [ ] E4.D3: Add an ADR for fabric secret storage, backup, restore, and ownership.
- [ ] E4.D4: Define Matter data-model-to-capability mapping and extension rules.
- [ ] E4.D5: Record supported commissioning transports and Thread/BLE limitations.

## Workstream E4.1: Feasibility spike

- [ ] Define a fixed evaluation matrix before choosing an implementation.
- [ ] Build each credible Rust controller candidate on macOS ARM and Linux x86_64.
- [ ] Commission at least one Matter-over-Wi-Fi test device.
- [ ] Read descriptors, endpoints, device types, clusters, and attributes.
- [ ] Subscribe to attribute and event changes.
- [ ] Invoke one reversible command and confirm the resulting observation.
- [ ] Restart the controller and prove fabric/node persistence.
- [ ] Measure first-party/non-Rust source and binary dependencies.
- [ ] Document maintenance activity, license, conformance status, and known gaps.
- [ ] Complete E4.D2 before production adapter implementation.

## Workstream E4.2: Controller and fabric lifecycle

- [ ] Implement a controller port independent of the selected Matter SDK.
- [ ] Create or load one HomeMagic-owned fabric.
- [ ] Store operational credentials through the secret-store boundary.
- [ ] Add commissioning-window discovery and setup-code validation.
- [ ] Add commission, cancel, remove-node, and decommission workflows.
- [ ] Persist node identity independently from mutable labels and network address.
- [ ] Add encrypted or otherwise protected fabric backup and tested restore.
- [ ] Surface attestation, certification, and trust failures explicitly.

## Workstream E4.3: Capability projection

- [ ] Map descriptor hierarchy into device and endpoint identities.
- [ ] Map On/Off to `on_off.v1`.
- [ ] Map Level Control to `level.v1`.
- [ ] Map Window Covering to `position.v1` with feature constraints.
- [ ] Map electrical measurement clusters to power/energy capabilities where the
  standard and SDK provide reliable semantics.
- [ ] Map Door Lock read/control semantics only after security policy review.
- [ ] Map battery, reachability, firmware, and diagnostics.
- [ ] Preserve unmapped standard/vendor data as versioned namespaced extensions.
- [ ] Add mapping fixtures derived from supported device types and feature maps.

## Workstream E4.4: Subscriptions and control

- [ ] Establish wildcard or targeted subscriptions with bounded resource use.
- [ ] Apply data-version and list semantics correctly.
- [ ] Reconcile after subscription loss and controller restart.
- [ ] Publish normalized observations and events through EPIC-001 contracts.
- [ ] Dispatch common capability commands only through EPIC-002.
- [ ] Translate interaction-model status into structured command outcomes.
- [ ] Prevent stale endpoint/feature assumptions after descriptor changes.
- [ ] Model sleepy-device availability without excessive polling.

## Workstream E4.5: Operations and compatibility

- [ ] Add RPC workflows for fabric status, commission, cancel, list nodes,
  decommission, backup, and restore.
- [ ] Add redacted diagnostics for controller, fabric, node, endpoint, and
  subscription state.
- [ ] Add OTA availability metadata without promising unsupported updates.
- [ ] Document IPv6, multicast, firewall, BLE, and Thread host requirements.
- [ ] Maintain a tested device/firmware/transport compatibility matrix.
- [ ] Add simulated Matter devices to CI where feasible.
- [ ] Define certification and release-readiness follow-up separately from
  functional compatibility.

## Test and verification checklist

- [ ] Mapping unit tests cover supported device types, feature maps, and missing
  optional attributes.
- [ ] Controller contract tests run against a simulated Matter device.
- [ ] Commission/decommission tests leave no orphaned durable identity.
- [ ] Restart tests prove fabric and node recovery.
- [ ] Subscription-loss tests prove reconciliation without duplicate identities.
- [ ] Backup/restore test uses a clean HomeMagic data directory.
- [ ] macOS ARM hardware test covers commissioning, observation, command, restart,
  and removal.
- [ ] Linux x86_64 hardware or reproducible lab evidence covers the same lifecycle.
- [ ] Non-Rust percentage and unsafe/FFI audit are repeatable from the repository.

## Acceptance criteria

- [ ] AC1: A supported Matter-over-Wi-Fi device can complete commission, restart,
  observe, command, and decommission lifecycle on both supported host targets.
- [ ] AC2: Node and endpoint identities remain stable across address changes and
  controller restart.
- [ ] AC3: Supported Matter behavior appears through common capabilities and the
  same command/policy path as Shelly.
- [ ] AC4: Subscription loss becomes visible and converges through reconciliation.
- [ ] AC5: Fabric backup restores control in a clean test environment without
  duplicating node identity.
- [ ] AC6: Any FFI or sidecar exception has an accepted ADR, narrow boundary,
  packaging tests, and replacement criteria.
- [ ] AC7: Compatibility claims name exact device, firmware, transport, host, and
  verified features.

## Exit gate

- [ ] All acceptance criteria contain linked evidence.
- [ ] Required ADRs are accepted and indexed.
- [ ] The 95% Rust measurement and dependency provenance report pass.
- [ ] Fabric secrets and diagnostics pass the security/redaction review.
- [ ] Unsupported Thread/BLE/certification cases are explicit in user docs.
- [ ] EPIC-005 can discover and operate Matter capabilities without Matter-specific
  tool schemas.

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Rust controller ecosystem lacks required features | Time-boxed spike and explicit exception ADR |
| Matter networking differs across hosts | Platform evidence and documented IPv6/multicast requirements |
| Cluster mapping leaks into the public model | Capability adapter plus namespaced extensions |
| Fabric loss makes devices unreachable | Protected backup/restore and clean-environment test |

## Progress log

- 2026-07-11: Epic created; feasibility may start before EPIC-002, production
  command delivery may not.
- 2026-07-12: User-approved SDK-neutral, simulation-first controller design
  committed. The first fixture slice is light plus door lock; unlock requires
  short-lived interactive authorization. Non-Rust reference tools are allowed
  only in development and CI.
- 2026-07-12: Eleven dependency-ordered issues planned across simulator,
  candidate, production-adapter, interoperability, and physical evidence. No
  hardware or production compatibility criterion is complete yet.
