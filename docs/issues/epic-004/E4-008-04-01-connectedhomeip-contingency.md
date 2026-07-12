---
id: E4-008-04-01
epic: EPIC-004
parent: E4-008-04
title: Audit the official ConnectedHomeIP contingency boundary
status: done
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

- [x] Pin the already discovered official release/commit and all submodules.
- [x] Build the controller/reference target reproducibly on macOS ARM64 and
  Linux x86_64 with on-network/BLE-disabled scope where supported.
- [x] Classify the independent commission/read/invoke/subscribe/restart/removal
  lifecycle as `not run` because no embeddable adapter exists; do not substitute
  CLI/tool evidence for the missing production boundary.
- [x] Inventory required libraries, processes, files, environment activation,
  binary size, build time, licenses, unsafe Rust, FFI calls, callbacks, and
  platform-specific packaging.
- [x] Define the narrowest versioned request/event ABI and process-crash
  behavior needed by `MatterController`; do not implement a broad generated
  binding surface.
- [x] Evaluate secret callbacks, cancellation, attestation, partial outcomes,
  and subscription loss and record the absent adapter proof as mandatory
  failures.
- [x] Record replacement/removal criteria and note that Rust share is unchanged
  until a first-party C++ adapter exists, whose exact impact must be measured.

## Verification

- [x] Both host reports contain build and boundary metrics; the audit records
  the independent lifecycle as explicitly `not run`.
- [x] No official SDK dependency enters a production manifest during the spike.
- [x] Every unsafe/FFI/process assumption has a boundary test or is a named
  mandatory failure.

## Progress log

- 2026-07-12: Source review at the exact release pin found a mature C++
  `DeviceCommissioner`, but no stable narrow C ABI. The existing Python binding
  is a broad callback-heavy C export surface owned by the Python controller,
  while `chip-tool` is a CLI/interactive test process. A two-host reproducible
  `chip-tool` no-BLE/no-Wi-Fi/no-Thread build audit now measures the actual
  bootstrap, source, binary, submodule, and prospective boundary cost before
  any exception can be proposed.
- 2026-07-12: The exact official release builds on both hosts, but no production
  adapter exists. The audit records the absent lifecycle/secret/cancellation
  boundary as mandatory failures instead of treating CLI commands as an
  embeddable controller proof. ConnectedHomeIP remains reference-only; the
  isolated matter.js contingency proceeds.
