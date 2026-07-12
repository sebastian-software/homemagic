# ADR-0038: Start with on-network commissioning and a fixed controller scorecard

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0005, ADR-0033, ADR-0034, EPIC-004, E4-001

## Context

Matter controller candidates vary in commissioning transports, protocol
coverage, persistence, native dependencies, platform support, maintenance, and
conformance claims. Testing a preferred candidate first would encourage scoring
rules that rationalize its gaps.

Commissioning a device already on an IP network is a different host requirement
from provisioning a new Wi-Fi or Thread device over Bluetooth LE. HomeMagic must
not imply BLE or Thread support from an on-network virtual-device test.

## Decision

The first accepted transport is on-network commissioning for a commissionable
Matter device already reachable on the local IP network, followed by normal
operational IPv6 communication. The initial external-reference lifecycle uses
this boundary.

BLE discovery/provisioning, `ble-wifi`/`code-wifi`, Thread operational-dataset
management, border-router ownership, `ble-thread`/`code-thread`, mobile handoff,
and multi-admin onboarding are unsupported until a later accepted ADR and
platform evidence. Setup QR/manual code parsing does not itself imply a
commissioning transport.

### Mandatory candidate gates

A production candidate is rejected unless evidence shows:

1. license and provenance are compatible with distribution;
2. it builds and runs on macOS ARM64 and Linux x86_64;
3. it can implement the SDK-neutral port without leaking SDK types;
4. it supports fabric create/load, on-network commissioning, inventory, bounded
   reads, invoke, subscriptions, restart persistence, and removal;
5. secret persistence can use ADR-0008 and ADR-0037 boundaries;
6. errors, cancellation, partial outcomes, and subscription loss are observable;
7. production packaging is reproducible and documented;
8. any unsafe, FFI, native library, sidecar, or non-Rust exception satisfies all
   ADR-0005 evidence and replacement requirements.

Failing one mandatory gate rejects the candidate regardless of weighted score.

### Fixed weighted scorecard

Candidates passing every gate are scored from committed reproducible evidence:

| Category | Weight |
| --- | ---: |
| Controller/protocol feature coverage and independent interoperability | 30 |
| Persistence, restart, subscriptions, and failure recovery | 20 |
| Rust share, unsafe/FFI/native dependency footprint | 20 |
| Cross-platform build, runtime, and packaging | 15 |
| Maintenance, security posture, license, and release discipline | 10 |
| Diagnostics, contract fit, and replacement cost | 5 |

Each category uses a published 0-5 rubric before candidate execution. Evidence
captures source revision, host, commands, fixtures, outputs, failures, binary and
dependency measurements, and known gaps.

Highest total score wins among candidates passing every gate. Scores within
three points prefer the stronger Rust/unsafe/FFI score; a remaining tie prefers
lower runtime/process and packaging complexity. If still tied, a pre-declared
time-boxed independent-reference spike decides. If no candidate passes, E4-008
records a blocker rather than reducing requirements implicitly.

ADR-0039 records the selected candidate or accepted narrow exception only after
the complete matrix is committed.

## Evidence classification

- a candidate's own examples/simulator are candidate-contract evidence;
- independently maintained virtual devices/tools are reference-interoperability
  evidence and remain development/CI-only;
- named physical devices require separate operator-authorized reports;
- certification is never inferred from any functional test.

## Consequences

- Candidate selection is reproducible rather than preference-driven.
- The first protocol lifecycle is feasible without BLE hardware.
- Many retail devices that require BLE Wi-Fi or Thread provisioning remain
  unsupported initially.
- Exact Nuki model, firmware, transport, and commissioning requirements must be
  verified before its later physical test.
- The scorecard cannot be changed after seeing results without a superseding ADR.

## References

- [CHIP Tool commissioning guide](https://project-chip.github.io/connectedhomeip-doc/development_controllers/chip-tool/chip_tool_guide.html)
