---
id: E4-007-02-03
epic: EPIC-004
parent: E4-007-02
title: Restore simulator exports without weakening production format checks
status: planned
priority: high
depends_on: [E4-007-02-01, E4-007-02-02]
adrs: [ADR-0033, ADR-0037]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-02-03: Simulator Restore Boundary

## Outcome

Simulator restore accepts sensitive `simulator_v1` input only on an explicitly
simulator-labelled path, while every production restore boundary continues to
reject that format before adapter code can inspect bytes.

## Tasks

- [ ] Admit restore intent without hashing or persisting envelope/key bytes.
- [ ] Require explicit simulator implementation and `simulator_v1` format.
- [ ] Stage reference-only metadata and transition restore phases durably.
- [ ] Activate metadata only after controller verification.
- [ ] Reject corrupt, wrong-key, wrong-fabric, duplicate-active, and production
  boundary misuse with structured outcomes.
- [ ] Preserve prior active state or create explicit repair evidence on partial
  restore.

## Acceptance criteria

- [ ] Simulator artifacts cannot cross a production restore path.
- [ ] Envelope and key are never ordinary persisted or diagnostic values.
- [ ] Failed restore cannot silently replace an active fabric.

## Verification

- [ ] Corrupt, wrong-key, conflict, restart, and successful reopen tests pass.
- [ ] Production format guard and secret-canary suites remain green.
