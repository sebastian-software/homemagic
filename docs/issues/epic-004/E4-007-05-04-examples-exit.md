---
id: E4-007-05-04
epic: EPIC-004
parent: E4-007-05
title: Validate Matter RPC examples and Track A exit evidence
status: ready
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

- [ ] Add schema-valid request/response/error examples for every method.
- [ ] Document sensitive setup/export/restore handling and non-replay behavior.
- [ ] Document cancellation, restart, partial cleanup, and repair procedures.
- [ ] Exercise light and lock behavior through common command RPC only.
- [ ] Produce a redacted cross-platform Track A exit report.

## Acceptance criteria

- [ ] Every documented example is executed or schema-validated in CI.
- [ ] Procedures match actual method names, errors, and operation phases.
- [ ] Exit evidence distinguishes simulator proof from production interoperability.

## Verification

- [ ] Full local gates and public Linux x86_64/macOS ARM64 CI pass.
