---
id: E4-011
epic: EPIC-004
title: Validate Matter operations, compatibility, and exit gates
status: planned
priority: critical
depends_on: [E4-010]
adrs: [ADR-0002, ADR-0005, ADR-0008, ADR-0014, ADR-0015, ADR-0033, ADR-0034, ADR-0035, ADR-0036, ADR-0037, ADR-0038, ADR-0039]
created: 2026-07-12
updated: 2026-07-12
---

# E4-011: Matter Exit Audit

## Outcome

Operators have safe, accurate Matter procedures and every EPIC-004 acceptance
and exit criterion links to exact simulation, adapter, reference, platform, or
physical evidence without overstating compatibility.

## Tasks

- [ ] Add commissioning, cancellation, repair, subscription, removal,
  export/restore, lost-vault, and controller-restart operator guidance.
- [ ] Extend the threat model for fabric ownership, setup input, attestation,
  unlock authorization, reference tooling, backup artifacts, and physical tests.
- [ ] Create a compatibility matrix keyed by exact device/reference fixture,
  firmware/revision, transport, host, adapter, and verified feature.
- [ ] Run deterministic contract and application suites on macOS ARM64 and Linux
  x86_64 and record normalized fixture hashes.
- [ ] Run production adapter and external-reference lifecycles on both supported
  targets, recording exact limitations instead of substituting evidence.
- [ ] Repeat Rust-share, unsafe/FFI, license, provenance, packaging, redaction,
  migration, and secret audits from a clean checkout.
- [ ] Prepare a Nuki-specific physical validation procedure only after recording
  exact model, firmware, Matter support, transport, installation context, safe
  actions, rollback, and cleanup.
- [ ] Obtain explicit user authorization immediately before any physical Nuki
  lock/unlock, commissioning, removal, or credential cleanup action.
- [ ] Run physical commission, observe, governed command, restart, resubscribe,
  remove, and cleanup on macOS ARM64 and Linux x86_64 as explicitly authorized.
- [ ] Record lock and unlock separately; a lock success cannot stand in for an
  authorized unlock test.
- [ ] Link every epic acceptance/exit checkbox to exact evidence and update the
  EPIC-005 handoff with only finalized common schemas and tools.

## Acceptance criteria

- [ ] Operators can inspect and recover every durable Matter operation without
  raw SDK or database manipulation.
- [ ] Compatibility claims name exact verified scope and preserve known gaps.
- [ ] Fabric secrets, unlock material, setup codes, and export keys remain
  redacted from every evidence artifact.
- [ ] Supported common capabilities use the same identity, observation, command,
  policy, event, and automation paths as non-Matter devices.
- [ ] Mac ARM64 and Linux x86_64 evidence satisfy the epic's complete lifecycle.
- [ ] Physical-device criteria remain unchecked until the supervised run exists.
- [ ] EPIC-004 and its issue index become `done` only when every linked criterion
  is complete; simulator/reference success alone is insufficient.

## Verification

- [ ] Repository-wide format, strict Clippy, full tests/all features, secret scan,
  migration fixtures, and docs links pass.
- [ ] Exit audit maps each AC and exit gate to one or more evidence files.
- [ ] Compatibility matrix entries reproduce from named runbooks and artifacts.
- [ ] Physical reports contain explicit user authorization context, host, device,
  firmware, actions, observed results, rollback, and cleanup status.
- [ ] No unsupported Thread, BLE, certification, OTA, or device claim appears as
  complete.

## Progress log

- 2026-07-12: Physical Nuki validation is intentionally pending and cannot be
  initiated from this planning approval.
