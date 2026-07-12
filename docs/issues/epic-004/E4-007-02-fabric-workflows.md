---
id: E4-007-02
epic: EPIC-004
parent: E4-007
title: Implement durable simulated fabric workflows
status: in_progress
priority: high
depends_on: [E4-007-01]
adrs: [ADR-0033, ADR-0037]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-02: Fabric Workflows

## Child issues

| Issue | Status | Outcome |
| --- | --- | --- |
| [E4-007-02-01](E4-007-02-01-fabric-status-create.md) | In progress | Idempotent staged fabric creation and status |
| [E4-007-02-02](E4-007-02-02-simulator-export.md) | In progress | Explicit sensitive simulator export |
| [E4-007-02-03](E4-007-02-03-simulator-restore-boundary.md) | In progress | Simulator restore and production-format rejection |

## Outcome

Authenticated operators can inspect and create the simulator fabric and perform
explicitly labelled simulator-only export and restore through durable,
idempotent operations with secret-safe input handling.

## Tasks

- [x] Implement fabric status and create orchestration.
- [x] Implement simulator export and restore with explicit evidence labels.
- [x] Keep export keys, protected envelopes, and controller state behind
  sensitive-value boundaries.
- [x] Return operation envelopes immediately and persist terminal evidence.
- [x] Reject simulator artifacts at production-format boundaries.

## Acceptance criteria

- [x] Fabric creation is idempotent per installation and request key.
- [x] Sensitive bytes never enter ordinary hashes, logs, events, or operation
  details.
- [x] Export and restore cannot be mistaken for production interoperability
  evidence.

## Verification

- [x] SQLite reopen, duplicate, invalid-key, corrupt-envelope, and redaction
  contracts pass.
- [x] Secret canaries are absent from database/WAL and redacted result surfaces.
- [x] Full local workspace gates pass.
- [ ] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: Decomposed into status/create, simulator export, and restore
  boundary slices. E4-007-02-01 is ready.
- 2026-07-12: Implemented all three child slices with schema 9 restart-safe
  secret staging, immediate actor-bound operations, explicit simulator evidence,
  non-serializable sensitive values, and fail-closed restart behavior. Targeted
  Matter and migration contracts, exact CI-format Clippy, boundary/secret scans,
  and the full privileged workspace test suite pass. Commit, push, and public CI
  remain pending because the local approval service reported its current usage
  limit.
