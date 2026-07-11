---
id: E3-003
epic: EPIC-003
title: Validate and compile bounded automation plans
status: done
priority: critical
depends_on: [E3-002]
adrs: [ADR-0017, ADR-0019]
created: 2026-07-11
updated: 2026-07-11
---

# E3-003: Validation and Compiler

## Tasks

- [x] Parse and structurally validate without side effects.
- [x] Resolve spaces, aliases, devices, endpoints, and capabilities at a registry revision.
- [x] Type-check values, variables, trigger fields, conditions, and commands.
- [x] Reject stale, missing, ambiguous, incompatible, cyclic, or impossible input.
- [x] Aggregate capability-specific Safety Profiles and approval requirements.
- [x] Compile deterministic node order and enforce every resource budget.
- [x] Reduce repeated same-target commands to the final desired state per segment.
- [x] Preserve intentional sequences across observable boundaries.
- [x] Return exact path/code/reason/remediation errors.

## Acceptance criteria

- [x] Invalid references or types prevent plan creation and identify exact paths.
- [x] `on -> off -> on` compiles to one `on` inside one segment.
- [x] A delay/wait/condition/event/dispatch boundary preserves intentional states.
- [x] Compilation is deterministic for the same document and registry revision.

## Evidence

- `AutomationCompiler` performs side-effect-free structural validation,
  reference resolution, type checking, Safety Profile derivation, desired-state
  reduction, and deterministic graph compilation.
- The normalized plan owns only stable device/endpoint targets, resolved
  triggers and expressions, and compiled fallback node references.
- Focused tests cover deterministic hashes, missing/ambiguous/stale/incompatible
  references, type errors, schedules, motion constraints, reduction, and delay
  boundaries.
- Published `automation.plan.v1` schema and typed example are validated by the
  domain test suite.
