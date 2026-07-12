---
id: E4-008-04-02
epic: EPIC-004
parent: E4-008-04
title: Audit the isolated matter.js sidecar fallback
status: done
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

- [x] Pin the discovered release/commit, package-manager lock, Node runtime, and
  transitive licenses.
- [x] Build/test on macOS ARM64 and Linux x86_64 without global tools; classify
  production packaging as a mandatory failure.
- [x] Run the same independent-reference lifecycle and record the bounded
  commissioning failures on both hosts.
- [x] Specify authentication, framing, backpressure, event cursors, secret
  transfer, process supervision, upgrade, rollback, and crash semantics.
- [x] Measure the development install footprint and record production footprint
  and first-party Rust-share impact as unproven mandatory failures.
- [x] Specify replacement through the SDK-neutral port and removal triggers;
  keep matter.js JSON/types out of public API.

## Verification

- [x] Reports distinguish build/import evidence, independent interop, and the
  absence of production HomeMagic protocol tests.
- [x] The spike keeps setup payloads out of reports and suppresses ordinary
  library logs; production redaction remains a mandatory failure.
- [x] The audit records missing runtime, incompatible sidecar, crash, hang, and
  downgrade handling as unimplemented mandatory failures.

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
- 2026-07-12: Exact two-host builds pass, but the independently maintained
  rs-matter fixture reaches `ArmFailSafe` and matter.js commissioning then
  exceeds a 180-second budget on both hosts. All downstream lifecycle phases
  remain `not_run`; the sidecar is rejected before scoring.
