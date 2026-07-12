---
id: E3-008
epic: EPIC-003
title: Validate automation operations and exit gate
status: done
priority: critical
depends_on: [E3-007]
adrs: [ADR-0017, ADR-0018, ADR-0019, ADR-0020, ADR-0021, ADR-0022, ADR-0023, ADR-0024, ADR-0025, ADR-0026, ADR-0027, ADR-0028, ADR-0029, ADR-0030, ADR-0031, ADR-0032]
created: 2026-07-11
updated: 2026-07-12
---

# E3-008: Automation Exit Audit

## Tasks

- [x] Add automation threat-model updates and operator recovery guide.
- [x] Document stuck runs, disable, rollback, trace, and explicit catch-up.
- [x] Add redacted end-to-end authored-document fixtures and evidence.
- [x] Run property, virtual-time, restart, parity, policy, retention, and secret gates.
- [x] Capture macOS ARM and Linux x64 quality evidence.
- [x] Link evidence to every EPIC-003 acceptance and exit criterion.
- [x] Update EPIC-005 with finalized lifecycle, schema, and RPC contracts.

## Acceptance criteria

- [x] Every active version is validated, simulated, and appropriately governed.
- [x] Every run identifies immutable version, trigger, decisions, and causation.
- [x] No arbitrary-code, raw-adapter, automatic missed-run, or secret escape exists.
- [x] Operators can safely inspect, stop, rollback, and deliberately catch up work.

## Evidence

- `docs/evidence/epic-003-exit-audit.md` links every acceptance and exit item.
- `docs/security/automation-threat-model.md` records trust boundaries, threats,
  controls, invariants, residual risks, and review triggers.
- `docs/operations/automation-recovery.md` gives exact RPC recovery procedures
  and distinguishes disable, cancel, rollback, compensation, catch-up, and
  retirement.
- macOS ARM and isolated Linux x86_64 gates passed format, strict Clippy, full
  tests/all features, doc tests, and migration fixtures; the secret scan passed
  on the identical checkout.
