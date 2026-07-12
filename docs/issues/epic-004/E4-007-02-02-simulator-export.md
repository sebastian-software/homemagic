---
id: E4-007-02-02
epic: EPIC-004
parent: E4-007-02
title: Export simulator fabric state through an explicit sensitive workflow
status: planned
priority: high
depends_on: [E4-007-02-01]
adrs: [ADR-0033, ADR-0037]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-02-02: Simulator Export

## Outcome

An authenticated export operation returns a clearly labelled `simulator_v1`
envelope and one-time recovery key only through a sensitive result; neither is
persisted, logged, hashed, or emitted as an ordinary event.

## Tasks

- [ ] Admit and persist export intent without sensitive output fields.
- [ ] Transition to `exporting` before calling the controller.
- [ ] Require the deterministic simulator implementation explicitly.
- [ ] Return envelope and recovery key in a redacted sensitive result type.
- [ ] Persist completed or structured failed/repair-required progress.

## Acceptance criteria

- [ ] Export format is always visibly `simulator_v1`.
- [ ] Retry never silently regenerates or persists a recovery key.
- [ ] Ordinary database backup still contains no usable fabric credentials.

## Verification

- [ ] Successful, missing-fabric, restart, duplicate, and redaction tests pass.
- [ ] Secret canaries are absent from SQLite, events, debug, and traces.
