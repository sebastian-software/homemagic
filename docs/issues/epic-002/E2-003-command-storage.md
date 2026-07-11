---
id: E2-003
epic: EPIC-002
title: Persist commands, idempotency, policy, and audit
status: done
priority: critical
depends_on: [E2-002]
adrs: [ADR-0007, ADR-0014]
created: 2026-07-11
updated: 2026-07-11
---

# E2-003: Command Storage

## Tasks

- [x] Add forward-only actor, grant, command, and audit migrations.
- [x] Persist command and immutable transition audit atomically.
- [x] Enforce actor-scoped idempotency keys and canonical request hashes.
- [x] Add optimistic state-machine version checks.
- [x] Add bounded query, recovery, and retention repository methods.
- [x] Test rollback, reopen, collision, ordering, and every non-terminal restart state.

## Acceptance criteria

- [x] No dispatchable command exists without durable actor and policy data.
- [x] Equivalent retries return one command; conflicting retries are rejected.
- [x] Audit history is append-only and causally ordered.

## Progress log

- 2026-07-11: Added schema v2 for actors, Argon2id credential hashes, grants,
  commands, actor-scoped idempotency, and independently retained audit history.
- 2026-07-11: Added the application-owned `CommandRepository` port and atomic
  SQLite implementation with optimistic locking, bounded recovery queries, and
  the 90-day/250,000 command plus 365-day/1,000,000 audit retention contracts.
- 2026-07-11: Storage tests cover rollback, reopen, equivalent and conflicting
  retries, immutable audit ordering, forged transitions, missing policy,
  retention, and all four non-terminal restart states.
