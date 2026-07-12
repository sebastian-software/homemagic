---
id: E4-007-05-04
epic: EPIC-004
parent: E4-007-05
title: Validate Matter RPC examples and Track A exit evidence
status: done
priority: high
depends_on: [E4-007-05-03]
adrs: [ADR-0003, ADR-0012, ADR-0016, ADR-0042]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-05-04: Examples and Exit Evidence

## Outcome

Executable schemas, JSON-RPC examples, and operator procedures prove the full
simulator-backed Matter lifecycle, sensitive exchange, cancellation, repair,
restart, common commands, and actor-filtered reconnect behavior.

## Tasks

- [x] Add schema-valid request/response/error examples for every method.
- [x] Document sensitive setup/export/restore handling and non-replay behavior.
- [x] Document cancellation, restart, partial cleanup, and repair procedures.
- [x] Exercise light and lock behavior through common command RPC only.
- [x] Produce a redacted cross-platform Track A exit report.

## Acceptance criteria

- [x] Every documented example is executed or schema-validated in CI.
- [x] Procedures match actual method names, errors, and operation phases.
- [x] Exit evidence distinguishes simulator proof from production interoperability.

## Verification

- [x] Full local gates pass.
- [x] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: Commit `a8e91c3` added one executable request/success/error example for all 17
  Matter methods, a shared JSON-RPC envelope schema, exact catalog validation,
  simulator recovery procedures, and the redacted Track A exit matrix. Existing
  common command RPC parity, simulator light/lock adapter, and exact unlock
  approval contracts form the command-boundary evidence. Full workspace tests,
  strict all-target Clippy, Matter boundaries, and disclosure scans pass;
  public CI remains pending.
- 2026-07-12: Public CI run `29209289949` passed Linux x86_64 Rust quality and
  Linux x86_64/macOS ARM64 simulator verification. This issue and E4-007-05 are
  done.
