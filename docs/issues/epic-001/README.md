---
id: EPIC-001-ISSUES
epic: EPIC-001
title: Reliable Device Foundation issue index
status: in_progress
priority: critical
depends_on: []
adrs: [ADR-0001, ADR-0002, ADR-0005, ADR-0006]
created: 2026-07-11
updated: 2026-07-11
---

# EPIC-001 Issue Index

Issues are completed in dependency order. Parallel work is allowed only where
the dependency graph and repository edits do not overlap.

| Issue | Status | Depends on | Outcome |
| --- | --- | --- | --- |
| [E1-001](E1-001-foundation-decisions.md) | Ready | — | Foundation ADRs |
| [E1-002](E1-002-device-lifecycle-contracts.md) | Planned | E1-001 | Domain contracts |
| [E1-003](E1-003-sqlite-storage.md) | Planned | E1-001, E1-002 | Durable repositories |
| [E1-004](E1-004-durable-reconciliation.md) | Planned | E1-003 | Load and reconcile |
| [E1-005](E1-005-shelly-authentication.md) | Planned | E1-001, E1-002 | Credential-safe auth |
| [E1-006](E1-006-shelly-managed-sessions.md) | Planned | E1-004, E1-005 | Live observations |
| [E1-007](E1-007-runtime-resilience.md) | Planned | E1-004, E1-006 | Bounded recovery |
| [E1-008](E1-008-read-api-and-repairs.md) | Planned | E1-003, E1-007 | Stable operational API |
| [E1-009](E1-009-operations-and-exit-audit.md) | Planned | E1-008 | Release evidence |

## Progress log

- 2026-07-11: Dependency-ordered issue set created from EPIC-001.
