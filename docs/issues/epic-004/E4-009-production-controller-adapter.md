---
id: E4-009
epic: EPIC-004
title: Integrate the selected production Matter controller
status: blocked
priority: critical
depends_on: [E4-005, E4-006, E4-008, E4-008-05]
adrs: [ADR-0005, ADR-0008, ADR-0033, ADR-0034, ADR-0037, ADR-0038, ADR-0039]
created: 2026-07-12
updated: 2026-07-12
---

# E4-009: Production Controller Adapter

## Outcome

The implementation selected by ADR-0039 satisfies the SDK-neutral controller
contract for the accepted Matter-over-Wi-Fi boundary on both supported targets,
with all SDK/native behavior isolated inside `homemagic-matter`.

## Tasks

- [ ] Pin the selected SDK/native dependencies and record provenance and license
  metadata.
- [ ] Implement fabric create/load and SDK persistence callbacks using only
  `SecretStore` references for secret material.
- [ ] Implement setup-code validation and accepted Matter-over-Wi-Fi discovery
  and commissioning flows.
- [ ] Implement descriptor, device-type, cluster, feature, attribute, and event
  inventory behind HomeMagic-owned types.
- [ ] Implement bounded reads, invokes, subscriptions, report normalization,
  data-version/list handling, resubscription, and restart recovery.
- [ ] Implement node removal and expose unknown/partial remote outcomes as repair
  state.
- [ ] Map supported On/Off and Door Lock behavior through the common projection
  and command paths.
- [ ] Add Level Control and Window Covering fixtures/projections when supported;
  retain calibration, feature, stop, freshness, and mechanical-policy gates.
- [ ] Expose battery, reachability, firmware, attestation/certification, OTA
  visibility, and diagnostics only where semantics are reliable.
- [ ] Isolate and document every accepted FFI/unsafe boundary with tests and
  replacement criteria.
- [ ] Compose the production adapter without making simulator/reference tooling a
  runtime dependency.

## Acceptance criteria

- [ ] The adapter passes every applicable controller contract test and marks
  intentionally unsupported operations with stable errors.
- [ ] No SDK type, raw command, callback, storage interface, or error escapes the
  integration crate.
- [ ] Live fabric secrets use ADR-0008 backends without plaintext fallback.
- [ ] Interaction acknowledgement is never reported as physical confirmation.
- [ ] Supported descriptors project to the same capability contracts as fixtures.
- [ ] macOS ARM64 and Linux x86_64 builds use the accepted packaging boundary.

## Verification

- [ ] Contract, fault, restart, resubscription, and partial-removal tests pass.
- [ ] Secret canaries are absent from storage, logs, diagnostics, and errors.
- [ ] SDK leakage and production dependency-graph audits pass.
- [ ] Rust-share, unsafe/FFI, license, and binary provenance reports match
  ADR-0039.
- [ ] Platform-specific adapter and unavailable-secret-backend tests pass.

## Progress log

- 2026-07-12: Planned behind evidence-based ADR-0039 selection.
- 2026-07-12: ADR-0039 selects no production controller because every candidate
  fails a mandatory gate. E4-008-05 must produce a passing boundary and a
  superseding ADR before this issue can become ready.
