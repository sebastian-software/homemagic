---
id: E4-007-02
epic: EPIC-004
parent: E4-007
title: Implement durable simulated fabric workflows
status: ready
priority: high
depends_on: [E4-007-01]
adrs: [ADR-0033, ADR-0037]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-02: Fabric Workflows

## Outcome

Authenticated operators can inspect and create the simulator fabric and perform
explicitly labelled simulator-only export and restore through durable,
idempotent operations with secret-safe input handling.

## Tasks

- [ ] Implement fabric status and create orchestration.
- [ ] Implement simulator export and restore with explicit evidence labels.
- [ ] Keep export keys, protected envelopes, and controller state behind
  sensitive-value boundaries.
- [ ] Return operation envelopes immediately and persist terminal evidence.
- [ ] Reject simulator artifacts at production-format boundaries.

## Acceptance criteria

- [ ] Fabric creation is idempotent per installation and request key.
- [ ] Sensitive bytes never enter ordinary hashes, logs, events, or operation
  details.
- [ ] Export and restore cannot be mistaken for production interoperability
  evidence.

## Verification

- [ ] SQLite reopen, duplicate, invalid-key, corrupt-envelope, and redaction
  contracts pass.
- [ ] Secret canaries are absent from database, diagnostics, and event streams.
