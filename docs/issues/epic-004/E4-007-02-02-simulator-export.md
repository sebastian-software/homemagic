---
id: E4-007-02-02
epic: EPIC-004
parent: E4-007-02
title: Export simulator fabric state through an explicit sensitive workflow
status: done
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

- [x] Admit and persist export intent without sensitive output fields.
- [x] Transition to `exporting` before calling the controller.
- [x] Require the deterministic simulator implementation explicitly.
- [x] Return envelope and recovery key in a redacted sensitive result type.
- [x] Persist completed or structured failed/repair-required progress.

## Acceptance criteria

- [x] Export format is always visibly `simulator_v1`.
- [x] Retry never silently regenerates or persists a recovery key.
- [x] Ordinary database backup still contains no usable fabric credentials.

## Verification

- [x] Successful, missing-fabric, restart, duplicate, and redaction tests pass.
- [x] Secret canaries are absent from SQLite, events, debug, and traces.
- [x] Full local workspace gates pass.
- [x] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: Implemented explicit simulator-only export, non-serializable
  sensitive output, one-time recovery material, and fail-closed restart
  handling. Targeted workflow and redaction contracts and the full local
  workspace gate pass; commit, push, and public CI remain pending.
- 2026-07-12: Public CI run `29202622965` passed the Linux x86_64 quality job
  and simulator hashes on Linux x86_64 and macOS ARM64. This child issue is
  done.
