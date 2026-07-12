---
id: E2-008
epic: EPIC-002
title: Validate hardware safety and command exit gate
status: in_progress
priority: critical
depends_on: [E2-007]
adrs: []
created: 2026-07-11
updated: 2026-07-12
---

# E2-008: Command Exit Audit

## Tasks

- [x] Add a command-control threat model and operator recovery guide.
- [ ] Add redacted switch, dimmer, and cover command reports with exact versions.
- [ ] Capture original state and restore every tested device after each scenario.
- [ ] Test emergency stop before other cover movement scenarios.
- [x] Run restart, timeout, retry, policy, audit, and secret-scan gates.
- [x] Link evidence to every EPIC-002 acceptance and exit criterion.
- [x] Update EPIC-003/004 with finalized command and policy contracts.

## Acceptance criteria

- [ ] Hardware cleanup is verified even when a scenario fails.
- [x] Unauthorized/unsafe commands cause no adapter dispatch.
- [x] Every accepted command has durable actor, policy, outcome, and audit evidence.

## Progress log

- 2026-07-11: Added a command-control threat model and operator recovery guide
  covering credentials, default deny, mechanical safety, duplicate prevention,
  restart ambiguity, compensation, physical stop, and incident evidence.
- 2026-07-11: Added an installation-bound query-based CLI for device execute
  grants; device-wide security grants remain intentionally unavailable.
- 2026-07-11: Added a redacted cleanup-first switch/dimmer/cover hardware harness.
  It validates by default, requires explicit physical-stop confirmation for cover
  execution, restores from `finally`, and cannot pass unverified cleanup.
- 2026-07-11: Added the EPIC-002 exit audit and finalized EPIC-003/004 dependency
  contracts. Automated gates pass; physical command reports remain pending.
- 2026-07-12: Revalidated the complete automated command suite on macOS ARM and
  isolated Linux x86_64. The supported-platform gate is closed; only explicitly
  supervised state-changing hardware reports and cleanup evidence remain.
