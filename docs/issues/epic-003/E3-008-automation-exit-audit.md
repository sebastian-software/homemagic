---
id: E3-008
epic: EPIC-003
title: Validate automation operations and exit gate
status: in_progress
priority: critical
depends_on: [E3-007]
adrs: [ADR-0017, ADR-0018, ADR-0019, ADR-0020]
created: 2026-07-11
updated: 2026-07-12
---

# E3-008: Automation Exit Audit

## Tasks

- [ ] Add automation threat-model updates and operator recovery guide.
- [ ] Document stuck runs, disable, rollback, trace, and explicit catch-up.
- [ ] Add redacted end-to-end authored-document fixtures and evidence.
- [ ] Run property, virtual-time, restart, parity, policy, retention, and secret gates.
- [ ] Capture macOS ARM and Linux x64 quality evidence.
- [ ] Link evidence to every EPIC-003 acceptance and exit criterion.
- [ ] Update EPIC-005 with finalized lifecycle, schema, and RPC contracts.

## Acceptance criteria

- [ ] Every active version is validated, simulated, and appropriately governed.
- [ ] Every run identifies immutable version, trigger, decisions, and causation.
- [ ] No arbitrary-code, raw-adapter, automatic missed-run, or secret escape exists.
- [ ] Operators can safely inspect, stop, rollback, and deliberately catch up work.
