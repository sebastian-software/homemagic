---
id: E4-007-04
epic: EPIC-004
parent: E4-007
title: Expose bounded diagnostics and subscription repair
status: ready
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
| [E4-007-04-01](E4-007-04-01-read-only-diagnostics.md) | Ready | Authenticated bounded read-only diagnostics |
| [E4-007-04-02](E4-007-04-02-subscription-status.md) | Planned | Deterministic freshness, budgets, and guidance |
| [E4-007-04-03](E4-007-04-03-explicit-subscription-repair.md) | Planned | Explicit gap-read and resubscribe workflow |
| [E4-007-04-04](E4-007-04-04-repair-restart-exhaustion.md) | Planned | Restart, exhaustion, retention, and exit evidence |

## Child issues

| Issue | Status | Outcome |
| --- | --- | --- |
| [E4-007-04-01](E4-007-04-01-bounded-diagnostics.md) | Ready | Read-only bounded redacted diagnostics |
| [E4-007-04-02](E4-007-04-02-explicit-subscription-repair.md) | Planned | Explicit gap-read and resubscribe orchestration |

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
- 2026-07-12: Decomposed into a read-only diagnostic slice followed by an
  explicit mutation slice so diagnostic access cannot imply repair authority.
  E4-007-04-01 is ready.
