# EPIC-004 Matter Controller Implementation Plan

- Date: 2026-07-12
- Status: Active
- Design: [Matter Controller Simulation-First Design](../superpowers/specs/2026-07-12-matter-controller-simulation-first-design.md)
- Issue index: [EPIC-004 issues](../issues/epic-004/README.md)

## Delivery rules

- Implement issues in dependency order and update issue status, checklists,
  progress, and evidence in the same commit as the work.
- Use English for code, ADRs, issues, API documentation, evidence, and commit
  messages.
- Use Conventional Commits and keep at least one independently verifiable
  commit per issue. Split an issue into multiple commits when decisions,
  migrations, behavior, and evidence are independently reviewable.
- Keep Matter inside the modular monolith and keep all SDK types behind the
  `MatterController` port.
- Route normal device actions through the shared EPIC-002 `CommandService`.
- Do not expose raw Matter cluster operations through RPC or MCP.
- Treat deterministic simulator, external reference, and physical hardware
  results as different evidence classes.
- Never check a production, interoperability, platform, or hardware criterion
  from simulator-only evidence.
- Preserve the 95%+ Rust target. Any FFI or sidecar exception requires the
  accepted ADR-0005 evidence before integration.

## Delivery tracks

### Track A: SDK-neutral simulation contract

E4-001 through E4-007 produce a complete simulator-backed vertical slice:
accepted boundaries, domain contracts, durable operations, deterministic light
and lock fixtures, capability projection, governed commands, authenticated RPC,
restart recovery, and cross-platform deterministic tests.

This track is independently useful and can finish without hardware. It does not
complete EPIC-004.

### Track B: protocol and hardware evidence

E4-008 through E4-011 evaluate current controller candidates, accept the
selection ADR, integrate the production adapter, prove external reference
interoperability, implement protected fabric portability, and finally collect
explicitly authorized physical-device evidence.

The Nuki hardware gate remains pending until the operator supplies the exact
model/firmware context and authorizes a supervised procedure.

## Sequence

### 1. E4-001 decisions and fixed evaluation contract

Accept ADRs for the SDK-neutral port, capability projection, interactive unlock
authorization, desired-state convergence, fabric ownership/portability, and the
initial transport plus candidate-evaluation boundary. Fix the candidate
scorecard before testing any implementation.

Verification: decision consistency review, placeholder scan, link audit, and
proof that no controller SDK has been selected by assumption.

### 2. E4-002 Matter domain and controller port

Add stable fabric/node/endpoint/projection identities, controller-owned value
types, reported/desired state contracts, redacted errors, operations, events,
and an async `MatterController` port. Add compile-time architecture tests that
prevent SDK types and raw protocol operations from entering public API crates.

Primary targets: `homemagic-domain`, `homemagic-application`.

Verification: serialization round trips, invalid transition tests, ID stability,
error redaction, object-safe port construction, and dependency-boundary checks.

### 3. E4-003 durable Matter metadata and operations

Add a forward-only SQLite migration and application-owned repository port for
fabrics, nodes, endpoints, projection revisions, desired/reported state,
subscriptions, long-running operations, unlock authorizations, and repair
records. Persist only `SecretRef` values for secret material.

Primary targets: `homemagic-application`, `homemagic-storage`.

Verification: migration fixtures, atomic operation transitions, restart
queries, supersession audit, single-use authorization consumption, retention,
and secret-canary scans.

### 4. E4-004 deterministic Rust controller simulator

Create `homemagic-matter` with an in-process implementation of the controller
port, virtual time and identities, versioned light and lock fixtures, dispatch
barriers, ordered fault injection, restart checkpoints, and a reusable contract
suite.

Primary targets: `homemagic-matter`, repository fixtures.

Verification: repeated byte-equivalent traces on macOS ARM64 and Linux x86_64,
complete fault scenarios, and proof that simulator credentials cannot become a
real fabric export.

### 5. E4-005 capability projection and subscription recovery

Project Descriptor/OnOff/DoorLock data into stable HomeMagic identities and
versioned capabilities. Normalize reports, data versions, descriptor changes,
subscription loss, bounded gap reads, staleness, and resubscription without
duplicate identities.

Primary targets: `homemagic-matter`, `homemagic-application`.

Verification: fixture mapping matrices, stale/out-of-order report tests,
descriptor-change invalidation, restart recovery, and bounded-resource tests.

### 6. E4-006 governed commands and access authorization

Implement Matter dispatch/confirmation behind `CommandService`, pre-dispatch
desired-state supersession, post-dispatch convergence, and short-lived,
single-use unlock authorization bound to actor, target, action, and desired
revision.

Primary targets: `homemagic-application`, `homemagic-matter`, command policy and
threat-model documentation.

Verification: light reduction, in-flight reconciliation, acknowledgement versus
observation, every unlock authorization failure mode, automation non-delegation,
restart, and audit-chain tests.

### 7. E4-007 simulator-backed workflows and RPC

Compose durable fabric, commissioning, cancellation, node removal, operation,
diagnostic, and subscription-repair workflows against the simulator. Expose
authenticated `matter.*` administration methods while retaining common device
and command methods for normal behavior.

Primary targets: `homemagic-application`, `homemagic-api`, `homemagic`.

Verification: SQLite-backed JSON-RPC lifecycle, actor isolation, redaction,
restart in every operation phase, partial cleanup visibility, and event-cursor
recovery.

### 8. E4-008 controller candidate evaluation and selection

Discover current credible Rust-native candidates, pin source revisions, run the
fixed scorecard on both supported targets, and record maintenance, licensing,
conformance claims, feature coverage, unsafe/FFI/native dependencies, packaging,
and replacement cost. Accept ADR-0039 only from linked evidence.

Primary targets: `docs/research`, reproducible spike scripts/examples,
cross-platform evidence, ADR-0039.

Verification: repeatable builds and contract results, provenance report,
first-party/non-Rust measurement, explicit gaps, and rejected-candidate reasons.

### 9. E4-009 production controller adapter

Implement the accepted SDK behind `MatterController`: fabric load/create,
Matter-over-Wi-Fi commissioning, descriptor reads, invoke, subscriptions,
bounded reads, restart, removal, and secret-store callbacks. Keep any accepted
FFI isolated and tested.

Primary targets: `homemagic-matter`, `homemagic-secrets`, daemon composition.

Verification: controller contract suite, adapter fault tests, no SDK type leaks,
secret redaction, packaging, and supported-platform builds.

### 10. E4-010 fabric portability and reference interoperability

Implement encrypted versioned fabric export/restore with explicit
authorization, clean-directory restore, duplicate-fabric protection, and
partial-failure recovery. Add a pinned development/CI-only external Matter
reference harness and prove the reproducible commission/read/subscribe/invoke/
restart/remove lifecycle without shipping the harness.

Primary targets: `homemagic-matter`, `homemagic-secrets`, `docs/operations`,
test tooling and CI.

Verification: clean-environment restore, secret-canary tests, reference lifecycle
on supported hosts where available, production dependency-graph inspection, and
documented IPv6/multicast/firewall requirements.

### 11. E4-011 operations and exit audit

Complete recovery, backup/restore, commissioning, decommissioning, and safe lock
validation guidance; update the compatibility matrix; capture macOS ARM64 and
Linux x86_64 evidence; run the Rust/FFI audit; and link every epic criterion to
exact evidence.

Physical Nuki validation is an explicit operator-supervised sub-gate. It remains
unchecked until exact model, firmware, transport, test actions, rollback, and
cleanup are recorded and the user authorizes that run.

## Dependency graph

```text
E4-001
  -> E4-002
       -> E4-003
       -> E4-004
            -> E4-005
                 -> E4-006
                      -> E4-007
            -> E4-008
E4-005 + E4-006 + E4-008
  -> E4-009
E4-007 + E4-009
  -> E4-010
       -> E4-011
```

E4-003 and E4-004 may proceed independently after E4-002. E4-008 may run after
the simulator contract is fixed while E4-005 through E4-007 complete.

## Repository-wide gate per implementation issue

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
./scripts/scan-secrets.sh
git diff --check
```

Issues that alter migrations also run migration fixture and reopen tests. Issues
that alter RPC run authenticated SQLite-backed transport tests. E4-008 onward
must additionally record macOS ARM64 and Linux x86_64 evidence separately.
