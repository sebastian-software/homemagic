# Matter Controller Simulation-First Design

- Status: Approved design awaiting repository review
- Date: 2026-07-12
- Epic: EPIC-004
- Depends on: EPIC-001 device lifecycle and EPIC-002 command control plane

## Purpose

HomeMagic needs Matter device compatibility without making Matter concepts part
of its public device model or committing the product to an immature controller
SDK too early. The first Matter increment must therefore establish a stable
controller boundary, exercise it with a deterministic Rust simulator, and use
the same contract to evaluate production controller candidates.

This design covers controller ownership, simulation, device projection,
commissioning operations, command convergence, lock authorization, secrets,
interoperability testing, and the evidence boundary for EPIC-004. It does not
claim physical compatibility before a physical device has been tested.

## Settled direction

1. Matter remains an integration behind the modular-monolith boundaries.
2. The application depends on an SDK-neutral `MatterController` port.
3. A deterministic in-process Rust simulator implements the same port first.
4. Production SDK candidates are evaluated independently against that contract.
5. Non-Rust reference tools are permitted only in development and CI.
6. The first simulated vertical slice contains a light and a door lock.
7. Unlocking always requires short-lived interactive authorization; locking may
   be automated through the normal policy path.
8. macOS ARM64 user processes and headless Linux x86_64 services are both
   first-class deployment models.
9. A real Nuki lock is a later, explicitly authorized hardware acceptance test,
   not evidence available to the initial implementation.

## Alternatives considered

### Bind the simulator to a Rust controller SDK immediately

This would reduce initial adapter work, but it would make the candidate's own
types, persistence model, and simulator behavior define the application
contract before cross-platform and interoperability evidence exists. It was
rejected because a self-compatible simulator is weak evidence for controller
selection and makes replacement harder.

### Use a non-Rust Matter server as the product runtime

An isolated server could provide a faster path to broad protocol behavior, but
it would add process supervision, packaging, version skew, a second security
boundary, and a substantial non-Rust runtime dependency. It remains a possible
time-boxed feasibility candidate only if native Rust candidates cannot satisfy
the fixed contract and ADR-0005 exception criteria.

### SDK-neutral port with an independent Rust simulator

This is the selected approach. It costs one explicit abstraction and simulator,
but lets application behavior stabilize independently, gives every candidate
the same acceptance contract, preserves a replacement boundary, and keeps
non-Rust reference implementations out of the shipped product.

## Architecture boundary

Matter support extends the existing modular monolith:

- `homemagic-domain` owns stable Matter fabric and node identities, projected
  capabilities, operation facts, desired/reported state, and diagnostic value
  types that contain no SDK types.
- `homemagic-application` owns commissioning workflows, projection,
  reconciliation, authorization, operation orchestration, and controller
  contract tests.
- `homemagic-storage` owns non-secret Matter metadata, operation state,
  mappings, and restart recovery.
- `homemagic-api` exposes authenticated Matter administration RPCs and existing
  capability-oriented query and command RPCs.
- a Matter integration adapter owns protocol-specific cluster, endpoint,
  subscription, and Interaction Model behavior.
- the composition root selects the simulator, a feasibility adapter, or the
  accepted production adapter. Domain and application crates never select an
  SDK directly.

The dependency direction is:

```text
RPC / MCP
    |
    v
application workflows -----> shared CommandService
    |                              |
    v                              v
MatterController port       Matter CommandDispatcher
    ^                              |
    |                              v
simulator or SDK adapter <--- protocol invocation and observation
```

`MatterController` is lower-level than the common device API but higher-level
than an SDK. It provides controller operations such as:

- create, inspect, load, export, and restore a HomeMagic-owned fabric;
- start, cancel, inspect, and recover commissioning;
- enumerate nodes, endpoints, device types, clusters, and feature metadata;
- establish, inspect, and restore subscriptions;
- perform bounded reads;
- invoke adapter-private protocol commands;
- remove a node and report incomplete cleanup explicitly.

The port uses HomeMagic-owned request, response, error, cursor, and event types.
SDK handles, cluster objects, callback types, storage interfaces, and error
enums cannot cross it. Protocol commands exposed by this port are accessible
only to the trusted Matter adapter; RPC and MCP callers cannot invoke them.

The Matter command adapter implements the EPIC-002 dispatch and confirmation
contracts. It translates a validated common capability command into a protocol
invocation and translates later reports or bounded reads into observed command
outcomes. An Interaction Model acknowledgement is not treated as physical
confirmation.

## Identity and capability projection

Matter identity remains independent from mutable labels and network addresses.
HomeMagic persists these relations:

```text
HomeMagic fabric ID -> Matter fabric identity
HomeMagic device ID -> fabric-scoped Matter node ID
HomeMagic endpoint ID -> node ID plus Matter endpoint number
HomeMagic capability ID -> endpoint plus versioned projection rule
```

The initial projection rules are deliberately small:

- an applicable On/Off cluster projects to `on_off.v1`;
- supported Level Control semantics project to `level.v1` in later fixtures;
- an applicable Door Lock cluster projects read state and the governed `lock`
  and `unlock` actions to a versioned access-control capability;
- reachability, battery, firmware, and diagnostics use existing common
  capabilities where their semantics are reliable;
- unmapped standard and vendor data remains read-only, versioned, and namespaced
  diagnostic data until an explicit projection rule is accepted.

Projection uses descriptor hierarchy, device type, server/client role, feature
maps, mandatory attributes, and available commands. Cluster presence alone is
not sufficient evidence that a capability is safe to expose. A descriptor or
feature-map change invalidates the affected cached projection and triggers
bounded rediscovery before another command can rely on it.

Display names, aliases, rooms, and agent-facing descriptions remain mutable
HomeMagic metadata. They never change fabric, node, endpoint, or capability
identity.

## Deterministic controller simulator

The first implementation is an in-process Rust simulator of the
`MatterController` port. It is a behavioral controller dependency for testing,
not an attempt to reimplement the Matter wire protocol.

The simulator contains:

- a virtual clock and deterministic identity source;
- a scripted fabric and node registry;
- virtual nodes, endpoints, descriptors, features, attributes, and commands;
- explicit dispatch barriers that let tests pause before or after invocation;
- ordered reports with configurable delay, duplication, loss, and reordering;
- subscription loss, reconnect, and resubscription behavior;
- controller and daemon restart checkpoints;
- partial commissioning, cancellation, removal, and restore failures;
- deterministic non-secret test credentials and redacted diagnostics.

The initial fixture contains:

1. a light with On/Off state and reversible commands;
2. a door lock with reported lock state and governed lock/unlock commands.

Fixtures are versioned repository artifacts. Given the same fixture, clock,
operation sequence, and injected failures, the simulator must produce the same
normalized operations, observations, commands, and events on macOS ARM64 and
Linux x86_64.

The simulator proves application semantics and controller contract behavior. It
does not prove IPv6, multicast DNS, BLE, transport timing, attestation,
certification, radio behavior, SDK correctness, or compatibility with a
physical product.

## Desired and reported state

Every projected stateful capability maintains at least:

- the latest desired state and its monotonic revision;
- the latest reported state, report version, and observation time;
- the last confirmed desired revision;
- freshness and convergence status;
- an optional structured uncertainty or failure reason.

Before dispatch, the shared command path may supersede older undispatched work
for the same device endpoint and capability. The latest desired revision wins.
The superseded command remains auditable and becomes `cancelled` with a stable
supersession reason and a reference to the replacing command. This uses the
existing ADR-0014 lifecycle rather than adding an adapter-only queue state.

Once a command has crossed the durable `dispatched` boundary, HomeMagic does not
claim that it can retract the physical effect. A newer desired revision causes
reconciliation toward the latest state after the in-flight outcome is observed
or becomes indeterminate. The event history records both the intermediate fact
and the final convergence.

Consequently, `light on -> off -> on` can become one `on` dispatch when all
three requests remain undispatched. If the first request was already dispatched,
HomeMagic guarantees only that reconciliation targets the final `on`; it does
not guarantee that the light never changed temporarily.

Coalescing and reconciliation live above the Matter SDK. They therefore behave
the same for the simulator, every candidate adapter, RPC commands, and
agent-authored automation commands.

## Door-lock authorization

Door Lock is an access-control capability and retains the default-deny security
classification from ADR-0015. The exact target grant remains necessary but is
not sufficient for `unlock`.

Every unlock command additionally references an interactive authorization that
is:

- issued only by an authenticated user with unlock-approval authority;
- bound to the exact device, endpoint, capability, `unlock` action, desired
  revision, and authenticated requesting actor;
- short-lived with an explicit expiry;
- single-use and consumed atomically before dispatch;
- invalid after target, payload, revision, actor, or policy changes;
- represented in audit and operation records only by opaque ID and decision
  metadata.

An automation cannot mint this authorization for itself. Approval of an
automation version does not pre-authorize future unlocks. `lock` still requires
the normal exact capability and target policy grant but does not require the
additional interactive authorization.

Simulator tests cover missing, expired, reused, mismatched, and valid unlock
authorization. No simulated success is recorded as physical Nuki evidence.

## Fabric persistence and secrets

ADR-0008 remains the live-secret boundary. Matter metadata stores only opaque
`SecretRef` values; private keys, operational certificates, fabric secrets,
setup codes, and export keys never enter ordinary SQLite rows, command
envelopes, events, diagnostics, fixtures, or logs.

Supported live-secret deployment models remain:

- macOS Keychain for a user process;
- Secret Service for a Linux desktop session;
- the explicitly configured encrypted file vault and separately provisioned
  master-key file for headless Linux.

There is no automatic fallback to plaintext or from a platform store to the
headless vault. Selective operating-system FFI remains isolated inside the
secret-store adapter under ADR-0005.

The Matter-specific ownership ADR must define:

- which component creates and owns the HomeMagic fabric and operational keys;
- atomic ordering between secret creation and metadata attachment;
- cleanup after failed commissioning and incomplete node removal;
- credential rotation and SDK storage callbacks;
- encrypted, versioned export and restore envelopes;
- explicit user authorization and key input for export and restore;
- clean-data-directory restore and duplicate-fabric protections;
- behavior when metadata exists but one or more secret references cannot be
  resolved.

Fabric export and restore are explicit interactive operations. HomeMagic never
silently exports a recovery copy and never reconstructs a missing vault. The
simulator uses deterministic placeholders that cannot be imported as a real
fabric.

## Durable commissioning and removal

Commissioning, cancellation, removal, export, and restore are durable
operations. Starting one returns an `operation_id` immediately. Each operation
records authenticated actor, input digest, phase, progress facts, correlation,
causation, timestamps, terminal outcome, and a redacted repair reason when
manual action is required.

A commissioning state machine distinguishes at least:

```text
requested
  -> validating_setup
  -> discovering
  -> establishing_session
  -> commissioning
  -> projecting
  -> subscribing
  -> completed | cancelled | failed | repair_required
```

Persisted checkpoints describe application progress but never cause blind
repetition of a protocol step whose remote outcome is unknown. Restart recovery
loads the SDK or simulator state, inspects the fabric and node, performs bounded
reads where safe, and then completes, fails, or exposes `repair_required`.

Cancellation is best effort. It prevents undispatched local phases but does not
claim to reverse remote work already completed. Removal similarly distinguishes
HomeMagic metadata removal, fabric node removal, and secret cleanup. A partial
result remains visible until an operator retries or acknowledges repair.

The first transport target is Matter over Wi-Fi on an already reachable local
network. BLE-assisted discovery, Thread commissioning, border-router ownership,
and mobile handoff remain explicit feasibility decisions and cannot be inferred
from simulator success.

## Subscriptions and recovery

The production adapter normalizes SDK callbacks into bounded controller events.
Application code owns ordering, deduplication, persistence, and projection.

Each subscription has a durable logical identity and ephemeral SDK/session
identity. Reports carry enough node, endpoint, attribute, data-version, and
sequence context to reject stale updates. List and data-version semantics must
be tested with fixed fixtures before their values update common capabilities.

Loss of a subscription is observable. Recovery performs jittered bounded
resubscription and a bounded read to close the notification gap. It does not
create a second HomeMagic identity or report stale cached state as fresh.
Sleepy devices use negotiated reporting and explicit staleness rather than
aggressive polling.

## RPC surface

Matter administration is RPC-shaped and authenticated. Exact request schemas
are finalized with the implementation ADR, but the intended method groups are:

- fabric status, create, export, and restore;
- commission start, cancel, and operation inspection;
- node list, detail, diagnostics, and removal;
- subscription status and repair diagnostics.

Long-running mutations return an operation envelope rather than holding the RPC
connection open. Setup codes, vault keys, private keys, and export passphrases
are accepted only through explicitly sensitive input handling and are redacted
before persistence or logging.

Normal observation and command calls continue to use common device,
capability, command, and event methods. Matter cluster IDs, attribute paths,
SDK operations, and raw writes are not public RPC or MCP tools.

## Candidate evaluation and interoperability

No production controller SDK is selected by this design. The feasibility issue
defines a fixed scorecard before running candidate spikes. Each credible
candidate is measured for:

- controller and commissioner completeness;
- Matter-over-Wi-Fi commissioning support;
- descriptor, read, invoke, event, and subscription behavior;
- persistence and fabric-key integration hooks;
- restart, resubscription, and error-reporting behavior;
- macOS ARM64 and Linux x86_64 build and runtime support;
- unsafe code, FFI, native libraries, binary size, and first-party Rust share;
- maintenance activity, licensing, release discipline, disclosed conformance,
  and replacement cost;
- ability to satisfy the SDK-neutral controller contract without leaking SDK
  types into application code.

A candidate cannot pass solely against its own simulated device stack. The
repository also provides a development/CI-only interoperability harness against
an external Matter reference implementation. These tools may use C++, Node.js,
or another ecosystem, but are pinned, provenance-recorded, isolated from
production packaging, and absent from runtime dependency graphs.

SDK selection, a narrow FFI exception, or an isolated sidecar requires an
accepted ADR with evidence. The default remains a native Rust adapter. Any
exception needs a removal criterion and cannot weaken the 95%+ Rust objective.

## Verification strategy

The controller contract suite runs against the deterministic simulator first
and later against every candidate and production adapter where the operation is
supported.

Required deterministic scenarios include:

- light discovery, projection, observation, command, and confirmation;
- lock discovery, projection, locking, and every unlock authorization outcome;
- command supersession before dispatch;
- a newer desired revision after dispatch and final reconciliation;
- delayed, duplicate, lost, and out-of-order reports;
- descriptor and feature-map changes before a command;
- subscription loss, bounded read, and resubscription;
- daemon restart during every durable operation phase;
- partial commissioning, cancellation, removal, export, and restore failures;
- missing secret references and unavailable secret backends;
- diagnostic and fixture secret-canary scans;
- stable identities across restart and address changes.

The same deterministic suite must pass on macOS ARM64 and Linux x86_64. The
external reference harness adds protocol interoperability evidence but is not a
substitute for physical-device evidence.

Every compatibility claim records exact device, firmware, transport, host,
adapter version, fixture or hardware source, verified capabilities, and known
gaps. The initial report must state that physical commissioning, local-network
discovery, physical lock motion, and Nuki compatibility are unverified.

## Milestone and evidence boundary

EPIC-004 is delivered through four independently checkable outcomes:

1. **Simulation contract:** controller port, light and lock fixtures, durable
   operations, recovery, policy, and cross-platform deterministic tests.
2. **Production adapter:** fixed candidate scorecard, accepted selection ADR,
   integrated SDK, persistence, subscriptions, and supported transport boundary.
3. **Reference interoperability:** pinned external reference environment and a
   reproducible commission/read/subscribe/invoke/restart/remove lifecycle.
4. **Physical acceptance:** explicitly authorized hardware runs on supported
   host targets with exact device and firmware evidence.

The first outcome can be completed without hardware. It must not mark the
production-adapter, reference-interoperability, hardware, or epic-level
acceptance boxes complete.

The real Nuki lock is reserved for a later operator-supervised test. Before that
test, the exact model, firmware, Matter capability, transport requirements,
safe test procedure, rollback, and cleanup steps must be recorded. Unlocking a
physical door requires explicit authorization at test time; ownership of the
device alone is not standing authorization.

## Required ADR work

Implementation planning must create or amend accepted ADRs for:

1. the SDK-neutral Matter controller port and adapter ownership;
2. the fixed candidate scorecard and final controller selection;
3. Matter fabric-key ownership, encrypted export, and restore on top of
   ADR-0008;
4. descriptor/cluster-to-capability projection and namespaced extensions;
5. supported commissioning transports and explicit BLE/Thread limitations;
6. single-use interactive authorization for access-control state changes;
7. desired-state supersession before dispatch and convergence after dispatch if
   existing command ADRs do not fully specify the shared behavior.

## Non-goals

- implementing the Matter wire protocol in the deterministic simulator;
- exposing raw cluster reads or writes as the main device API;
- selecting a controller SDK without the fixed feasibility evidence;
- claiming CSA certification or comprehensive device compatibility;
- shipping development reference tools;
- Matter bridge/server behavior;
- complete Thread or BLE commissioning in the first slice;
- automatic fabric recovery, secret fallback, or silent missed-operation replay;
- physical Nuki operation without a separate authorized validation run.

## Review checklist

- [ ] The port boundary can be implemented without SDK types in domain or
  application crates.
- [ ] Simulator evidence is never presented as protocol or hardware evidence.
- [ ] Light and lock fixtures cover reversible and access-control behavior.
- [ ] Unlock authorization is exact, short-lived, single-use, and non-delegable
  to an automation.
- [ ] Desired-state supersession preserves durable command audit history.
- [ ] ADR-0008 remains the only live-secret backend policy.
- [ ] Headless Linux never depends on an active desktop secret service.
- [ ] Development reference tools cannot enter production packaging.
- [ ] macOS ARM64 and Linux x86_64 evidence is independently recorded.
- [ ] Physical Nuki and complete EPIC-004 acceptance remain explicitly pending.
