---
id: E3-003
epic: EPIC-003
title: Validate and compile bounded automation plans
status: planned
priority: critical
depends_on: [E3-002]
adrs: [ADR-0017, ADR-0019]
created: 2026-07-11
updated: 2026-07-11
---

# E3-003: Validation and Compiler

## Tasks

- [ ] Parse and structurally validate without side effects.
- [ ] Resolve spaces, aliases, devices, endpoints, and capabilities at a registry revision.
- [ ] Type-check values, variables, trigger fields, conditions, and commands.
- [ ] Reject stale, missing, ambiguous, incompatible, cyclic, or impossible input.
- [ ] Aggregate capability-specific Safety Profiles and approval requirements.
- [ ] Compile deterministic node order and enforce every resource budget.
- [ ] Reduce repeated same-target commands to the final desired state per segment.
- [ ] Preserve intentional sequences across observable boundaries.
- [ ] Return exact path/code/reason/remediation errors.

## Acceptance criteria

- [ ] Invalid references or types prevent plan creation and identify exact paths.
- [ ] `on -> off -> on` compiles to one `on` inside one segment.
- [ ] A delay/wait/condition/event/dispatch boundary preserves intentional states.
- [ ] Compilation is deterministic for the same document and registry revision.
