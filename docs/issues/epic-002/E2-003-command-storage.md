---
id: E2-003
epic: EPIC-002
title: Persist commands, idempotency, policy, and audit
status: planned
priority: critical
depends_on: [E2-002]
adrs: [ADR-0007, ADR-0014]
created: 2026-07-11
updated: 2026-07-11
---

# E2-003: Command Storage

## Tasks

- [ ] Add forward-only actor, grant, command, and audit migrations.
- [ ] Persist command and immutable transition audit atomically.
- [ ] Enforce actor-scoped idempotency keys and canonical request hashes.
- [ ] Add optimistic state-machine version checks.
- [ ] Add bounded query, recovery, and retention repository methods.
- [ ] Test rollback, reopen, collision, ordering, and every non-terminal restart state.

## Acceptance criteria

- [ ] No dispatchable command exists without durable actor and policy data.
- [ ] Equivalent retries return one command; conflicting retries are rejected.
- [ ] Audit history is append-only and causally ordered.
