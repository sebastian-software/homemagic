---
id: E4-007-02-03
epic: EPIC-004
parent: E4-007-02
title: Restore simulator exports without weakening production format checks
status: done
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

- [x] Admit restore intent without hashing or persisting envelope/key bytes.
- [x] Require explicit simulator implementation and `simulator_v1` format.
- [x] Stage reference-only metadata and transition restore phases durably.
- [x] Activate metadata only after controller verification.
- [x] Reject corrupt, wrong-key, wrong-fabric, duplicate-active, and production
  boundary misuse with structured outcomes.
- [x] Preserve prior active state or create explicit repair evidence on partial
  restore.

## Acceptance criteria

- [x] Simulator artifacts cannot cross a production restore path.
- [x] Envelope and key are never ordinary persisted or diagnostic values.
- [x] Failed restore cannot silently replace an active fabric.

## Verification

- [x] Corrupt, wrong-key, conflict, restart, and successful reopen tests pass.
- [x] Production format guard and secret-canary suites remain green.
- [x] Full local workspace gates pass.
- [x] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: Implemented sensitive simulator restore input, production-format
  rejection, conflict and corruption outcomes, and restart reconciliation
  without sensitive-input reuse. Targeted workflow and canary contracts and the
  full local workspace gate pass; commit, push, and public CI remain pending.
- 2026-07-12: Public CI run `29202622965` passed the Linux x86_64 quality job
  and simulator hashes on Linux x86_64 and macOS ARM64. This child issue is
  done.
