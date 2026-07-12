---
id: E4-007-04
epic: EPIC-004
parent: E4-007
title: Expose bounded diagnostics and subscription repair
status: planned
priority: high
depends_on: [E4-007-01, E4-007-03]
adrs: [ADR-0033, ADR-0034]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-04: Subscription Diagnostics and Repair

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
