---
id: E3-002
epic: EPIC-003
title: Define the versioned automation domain and schema
status: planned
priority: critical
depends_on: [E3-001]
adrs: [ADR-0017, ADR-0018, ADR-0019]
created: 2026-07-11
updated: 2026-07-11
---

# E3-002: Automation Domain

## Tasks

- [ ] Add stable automation, version, run, occurrence, timer, and trace IDs.
- [ ] Define immutable version documents with provenance and schema version.
- [ ] Define every approved trigger, condition, action, variable, and run mode.
- [ ] Define lifecycle, run, occurrence, timer, and approval state machines.
- [ ] Define normalized execution plan, resource budgets, and stable node order.
- [ ] Define machine-readable validation errors with JSON Pointer paths.
- [ ] Define canonical hashing and compatibility rules.
- [ ] Publish JSON schema and representative fixtures.
- [ ] Add round-trip, lifecycle, canonicalization, bound, and property tests.

## Acceptance criteria

- [ ] Unknown schema versions and unbounded constructs are impossible to execute.
- [ ] No domain type represents arbitrary code or a raw adapter operation.
- [ ] Equivalent documents normalize and hash identically.
- [ ] Persisted public contracts round-trip across the current schema version.
