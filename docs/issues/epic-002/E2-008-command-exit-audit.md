---
id: E2-008
epic: EPIC-002
title: Validate hardware safety and command exit gate
status: planned
priority: critical
depends_on: [E2-007]
adrs: []
created: 2026-07-11
updated: 2026-07-11
---

# E2-008: Command Exit Audit

## Tasks

- [ ] Add a command-control threat model and operator recovery guide.
- [ ] Add redacted switch, dimmer, and cover command reports with exact versions.
- [ ] Capture original state and restore every tested device after each scenario.
- [ ] Test emergency stop before other cover movement scenarios.
- [ ] Run restart, timeout, retry, policy, audit, and secret-scan gates.
- [ ] Link evidence to every EPIC-002 acceptance and exit criterion.
- [ ] Update EPIC-003/004 with finalized command and policy contracts.

## Acceptance criteria

- [ ] Hardware cleanup is verified even when a scenario fails.
- [ ] Unauthorized/unsafe commands cause no adapter dispatch.
- [ ] Every accepted command has durable actor, policy, outcome, and audit evidence.
