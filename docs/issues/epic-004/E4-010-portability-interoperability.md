---
id: E4-010
epic: EPIC-004
title: Prove fabric portability and external reference interoperability
status: planned
priority: critical
depends_on: [E4-007, E4-009]
adrs: [ADR-0005, ADR-0008, ADR-0037, ADR-0038, ADR-0039]
created: 2026-07-12
updated: 2026-07-12
---

# E4-010: Portability and Reference Interoperability

## Outcome

HomeMagic can explicitly export and restore its fabric through a protected,
versioned process, and the production adapter completes a reproducible lifecycle
against an independently maintained Matter reference implementation that is not
shipped with the product.

## Tasks

- [ ] Implement a versioned encrypted export envelope that includes the minimum
  fabric material and integrity-bound metadata required by ADR-0037.
- [ ] Require explicit authenticated authorization and sensitive key input for
  export and restore; never place key material in ordinary RPC persistence.
- [ ] Restore into a clean HomeMagic data directory while preserving stable
  fabric/node identity and preventing duplicate active ownership.
- [ ] Model interrupted export, wrong key, corrupt envelope, missing secret,
  partial restore, and already-active-fabric outcomes explicitly.
- [ ] Verify export artifacts and database backups have distinct documented
  security properties.
- [ ] Select and pin an independent Matter reference tool from current primary
  sources for development/CI only.
- [ ] Script reproducible virtual-device commission, inventory, subscribe, read,
  invoke, controller restart, node removal, and cleanup.
- [ ] Cover On/Off and Door Lock; add Level Control and Window Covering where the
  selected reference environment provides reliable fixtures.
- [ ] Record exact reference revision, configuration, transport, host, adapter,
  features, commands, outputs, and known gaps.
- [ ] Document IPv6, multicast DNS, firewall, interface, BLE, Thread, and
  border-router requirements and unsupported cases.
- [ ] Prove reference dependencies, containers, Node/C++ runtimes, and test
  credentials are absent from production artifacts.

## Acceptance criteria

- [ ] A clean-directory restore regains control without duplicating identity.
- [ ] Wrong or missing recovery input fails closed without damaging the active
  fabric.
- [ ] The independent reference lifecycle is repeatable from committed tooling.
- [ ] Reference success is labeled interoperability evidence, not certification
  or named-device compatibility.
- [ ] Production runtime and packages contain no external reference server/tool.

## Verification

- [ ] Export/restore round-trip, corruption, wrong-key, duplicate, and crash
  matrices pass.
- [ ] Secret-canary and redacted-diagnostic scans pass for every artifact.
- [ ] Reference lifecycle passes on macOS ARM64 and Linux x86_64 or records an
  explicit tool/host limitation without substituting simulator evidence.
- [ ] Production dependency, package-content, process, and network-port audits
  prove the harness is absent.
- [ ] Clean-environment runbook reproduces the recorded result.

## Progress log

- 2026-07-12: Non-Rust tooling is allowed only inside this development/CI
  evidence boundary.
