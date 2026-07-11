---
id: E3-002
epic: EPIC-003
title: Define the versioned automation domain and schema
status: done
priority: critical
depends_on: [E3-001]
adrs: [ADR-0017, ADR-0018, ADR-0019]
created: 2026-07-11
updated: 2026-07-11
---

# E3-002: Automation Domain

## Tasks

- [x] Add stable automation, version, run, occurrence, timer, and trace IDs.
- [x] Define immutable version documents with provenance and schema version.
- [x] Define every approved trigger, condition, action, variable, and run mode.
- [x] Define lifecycle, run, occurrence, timer, and approval state machines.
- [x] Define normalized execution plan, resource budgets, and stable node order.
- [x] Define machine-readable validation errors with JSON Pointer paths.
- [x] Define canonical hashing and compatibility rules.
- [x] Publish JSON schema and representative fixtures.
- [x] Add round-trip, lifecycle, canonicalization, bound, and property tests.

## Acceptance criteria

- [x] Unknown schema versions and unbounded constructs are impossible to execute.
- [x] No domain type represents arbitrary code or a raw adapter operation.
- [x] Equivalent documents normalize and hash identically.
- [x] Persisted public contracts round-trip across the current schema version.

## Progress log

- 2026-07-11: Added opaque automation/run/occurrence/timer/trace/approval IDs,
  positive immutable versions, canonical SHA-256 content hashes, and explicit
  document/plan schemas.
- 2026-07-11: Added the complete declarative IR, normalized plan contract,
  capability Safety Profiles, hard resource budgets, lifecycle state machines,
  durable run/timer/occurrence/approval/trace records, and JSON Pointer findings.
- 2026-07-11: Published and machine-validated the v1 JSON Schema plus a
  comprehensive agent-authored example. Property and persisted-contract tests
  prove round trips, canonical map ordering, monotonic versions, and bound
  rejection; all domain tests and strict Clippy pass.
