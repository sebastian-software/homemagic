---
id: E4-008-05
epic: EPIC-004
title: Resolve the Matter controller selection blocker
status: ready
priority: critical
depends_on: [E4-008-04]
adrs: [ADR-0005, ADR-0038, ADR-0039]
created: 2026-07-12
updated: 2026-07-12
---

# E4-008-05: Controller Selection Remediation

## Outcome

At least one candidate passes every unchanged ADR-0038 mandatory gate, or the
blocker is narrowed to an upstream protocol defect with a reproducible fixture
and no production compatibility claim.

## Tasks

- [x] Add secret-safe commissioning-stage instrumentation around matter.js and
  record the last completed Matter stage before the independent timeout.
- [x] Build a pinned official ConnectedHomeIP on-network all-clusters/light
  device as a second independent fixture on both hosts.
- [x] Run the same commission, inventory, read, subscribe, invoke, process
  restart, and removal lifecycle against rs-matter and the official fixture.
- [x] Classify any implementation-specific incompatibility without weakening
  the requirement for at least one complete independent lifecycle per host.
- [ ] If matter.js passes, implement the inherited-pipe private protocol,
  reverse Rust-owned secret driver, cancellation, partial outcomes, event
  windows, supervision, and SDK-neutral contract suite.
- [ ] Produce pruned signed/runtime-package evidence with bundled pinned Node,
  license closure, upgrade/rollback, crash, missing-runtime, downgrade, and
  secret-canary tests.
- [ ] If matter.js still fails, time-box the smallest ConnectedHomeIP opaque C
  adapter or upstream Rust fixes and repeat the same gates.
- [ ] Accept a superseding controller-selection ADR only after all evidence is
  committed and both host jobs pass.

## Verification

- [ ] No setup payload, fabric key, or certificate private material appears in
  logs, reports, crash output, or ordinary persistence.
- [ ] Candidate self-tests remain separate from independent interoperability.
- [ ] Production Cargo manifests remain free of rejected/reference tooling.
- [ ] E4-009 stays blocked until a superseding ADR names one passing boundary.

## Progress log

- 2026-07-13: The second Rust boundary slice adds zeroizing/redacted secret
  values, typed reverse get/put/delete/compare-and-swap dispatch, stable backend
  outcomes, cancellation that never claims a dispatched mutation was stopped,
  partial-disconnect classification, and a contiguous bounded event window.
  Secret-shape, optimistic-conflict, redaction, cancellation, gap, duplicate,
  exhaustion, acknowledgement, and no-coalesce contract tests pass under strict
  Clippy.
- 2026-07-13: The Rust adapter now owns a first executable private-protocol
  slice: bounded big-endian framing, runtime/version/capability negotiation,
  nonce-bound envelopes, stable result/partial/error dispositions, event/ack
  shapes, and payload-wide diagnostic redaction. Matter-crate contract tests and
  strict all-target Clippy pass; reverse secrets, flow-control state, and process
  supervision remain deliberately unchecked.
- 2026-07-13: Test-only run `29214794784` seeds the already known on-network
  address directly into matter.js immediately before operational CASE. Both
  hosts then pass commission, inventory, read, toggle, subscription, controller
  restart, and removal. Combined with unmodified run `29213972862`, this proves
  macOS Matter credentials and CASE are functional and isolates the remaining
  failure to operational mDNS address acquisition in the self-hosted fixture
  topology. The diagnostic patch is not a production workaround and normal
  push CI remains unmodified.
- 2026-07-12: Public run `29213972862` builds pinned ConnectedHomeIP
  `v1.5.1.0` light fixtures and runs the same lifecycle on both hosts. Linux
  x86_64 passes commission, inventory, read, toggle, subscription, controller
  restart, and removal. macOS ARM64 reaches the same `18.1 Reconnect` boundary
  as the rs-matter fixture and times out. This excludes the rs-matter fixture as
  the common cause and narrows remediation to matter.js operational
  discovery/CASE behavior on macOS or its host environment.
- 2026-07-12: The matter.js spike now wraps each upstream commissioning step
  and persists only step number, static step name, status, and error class. A
  two-host official ConnectedHomeIP light workflow is running as the second
  independent fixture; setup values remain absent from reports and normal logs.
- 2026-07-12: Public run `29213726913` records the same boundary on both hosts:
  steps through attestation, certificates/NOC, and access control complete;
  commissioning stalls at `18.1 Reconnect`, the first operational CASE
  connection.
