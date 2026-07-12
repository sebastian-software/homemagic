---
id: E4-008-04-02
epic: EPIC-004
parent: E4-008-04
title: Audit the isolated matter.js sidecar fallback
status: in_progress
priority: high
depends_on: [E4-008-04-01]
adrs: [ADR-0005, ADR-0038]
created: 2026-07-12
updated: 2026-07-12
---

# E4-008-04-02: matter.js Sidecar Contingency

## Outcome

If the narrower official-SDK boundary fails, the pinned `matter.js` controller
is measured as a separately sandboxed process with a versioned private protocol
and no Node/TypeScript dependency in HomeMagic production crates.

## Tasks

- [ ] Pin the discovered release/commit, package-manager lock, Node runtime, and
  transitive licenses.
- [ ] Build/test/package on macOS ARM64 and Linux x86_64 without global tools.
- [ ] Run the same independent-reference lifecycle and fixed failure cases.
- [ ] Specify authentication, framing, backpressure, event cursors, secret
  transfer, process supervision, upgrade, rollback, and crash semantics.
- [ ] Measure installed/runtime footprint and first-party Rust-share impact.
- [ ] Demonstrate replacement by the SDK-neutral port and define removal
  triggers; do not expose matter.js JSON/types as public API.

## Verification

- [ ] Reports distinguish sidecar self-tests, independent interop, and
  HomeMagic protocol tests.
- [ ] Secrets and setup payloads are absent from ordinary logs and persistence.
- [ ] Missing Node runtime, incompatible sidecar, crash, hang, and protocol
  downgrade fail closed with stable controller errors.

## Progress log

- 2026-07-12: Source review at the exact pin confirmed a complete
  `CommissioningController` with on-network commissioning, inventory, read,
  invoke, automatic subscriptions, restart persistence, removal, and discovery
  cancellation. The isolated two-host audit pins Node and the upstream lockfile
  and measures build/runtime footprint before any private sidecar protocol is
  accepted.
- 2026-07-12: The proposed private boundary now fixes inherited-pipe framing,
  handshake/version rules, reverse Rust-owned secret callbacks, event-window
  backpressure, cancellation/partial outcomes, supervision, packaging, and
  removal tests. It remains unaccepted until lifecycle and fault evidence pass.
