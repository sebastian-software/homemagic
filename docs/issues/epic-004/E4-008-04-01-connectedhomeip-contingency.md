---
id: E4-008-04-01
epic: EPIC-004
parent: E4-008-04
title: Audit the official ConnectedHomeIP contingency boundary
status: ready
priority: critical
depends_on: [E4-008-03]
adrs: [ADR-0005, ADR-0038]
created: 2026-07-12
updated: 2026-07-12
---

# E4-008-04-01: ConnectedHomeIP Contingency

## Outcome

The exact official SDK release is built and exercised on both target hosts, and
the smallest viable process or C ABI boundary is measured without placing C++
types, ownership, callbacks, or exceptions in HomeMagic public contracts.

## Tasks

- [ ] Pin the already discovered official release/commit and all submodules.
- [ ] Build the controller/reference target reproducibly on macOS ARM64 and
  Linux x86_64 with on-network/BLE-disabled scope where supported.
- [ ] Exercise commission, read, invoke, subscribe, restart, and removal against
  an independent fixture; distinguish CLI/tool evidence from an embeddable API.
- [ ] Inventory required libraries, processes, files, environment activation,
  binary size, build time, licenses, unsafe Rust, FFI calls, callbacks, and
  platform-specific packaging.
- [ ] Define the narrowest versioned request/event ABI and process-crash
  behavior needed by `MatterController`; do not implement a broad generated
  binding surface.
- [ ] Prove secret callbacks, cancellation, attestation, partial outcomes, and
  subscription loss can remain observable through that boundary.
- [ ] Record replacement/removal criteria and the exact first-party Rust-share
  impact required by ADR-0005.

## Verification

- [ ] Both host reports contain commands, outputs, binary/dependency metrics,
  and independent-reference outcomes.
- [ ] No official SDK dependency enters a production manifest during the spike.
- [ ] Every unsafe/FFI/process assumption has a boundary test or is a named
  mandatory failure.
