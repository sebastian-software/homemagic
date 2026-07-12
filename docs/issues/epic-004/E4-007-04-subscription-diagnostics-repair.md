---
id: E4-007-04
epic: EPIC-004
parent: E4-007
title: Expose bounded diagnostics and subscription repair
status: in_progress
priority: high
depends_on: [E4-007-01, E4-007-03]
adrs: [ADR-0033, ADR-0034, ADR-0041]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-04: Subscription Diagnostics and Repair

## Child issues

| Issue | Status | Outcome |
| --- | --- | --- |
| [E4-007-04-01](E4-007-04-01-read-only-diagnostics.md) | Done | Authenticated bounded read-only diagnostics |
| [E4-007-04-02](E4-007-04-02-subscription-status.md) | Ready | Deterministic freshness, budgets, and guidance |
| [E4-007-04-03](E4-007-04-03-explicit-subscription-repair.md) | Planned | Explicit gap-read and resubscribe workflow |
| [E4-007-04-04](E4-007-04-04-repair-restart-exhaustion.md) | Planned | Restart, exhaustion, retention, and exit evidence |

## Outcome

Operators can inspect bounded redacted fabric, node, endpoint, operation, and
subscription state and explicitly repair recoverable subscription gaps without
raw protocol mutation access.

## Tasks

- [ ] Implement bounded fabric/node/endpoint and controller diagnostics.
- [ ] Implement subscription status with freshness, retry, and repair state.
- [ ] Add explicit resubscribe and bounded gap-read repair workflows.
- [ ] Persist repair attempts and outcomes through administration operations.
- [ ] Redact native identifiers, network material, setup data, and secrets.

## Acceptance criteria

- [ ] Diagnostics cannot trigger writes or expose raw cluster write methods.
- [ ] Repair is explicit and never inferred as automatic catch-up permission.
- [ ] Exhausted repair remains visible with stable remediation guidance.

## Verification

- [ ] Pagination, redaction, retry exhaustion, restart, and repaired-gap tests
  pass.
- [ ] Diagnostic secret-canary scans remain clean.

## Progress log

- 2026-07-12: E4-007-03 completed with public cross-platform CI. This issue is
  ready.
- 2026-07-12: Decomposed into four dependency-ordered slices. ADR-0041 fixes
  the read-only diagnostics versus explicit repair boundary; E4-007-04-01 is
  ready.
- 2026-07-12: E4-007-04-01 is implemented and local CI-equivalent gates pass;
  controller call-count evidence and public CI remain pending.
- 2026-07-12: E4-007-04-01 completed with public CI run `29206011230` green
  across Linux x86_64 and macOS ARM64. E4-007-04-02 is ready.
