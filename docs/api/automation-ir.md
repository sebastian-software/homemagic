# Automation Document and Plan Contracts

## Published contracts

- Authored document schema: [`automation.document.v1`](schemas/automation-document.v1.schema.json)
- Comprehensive authored example: [`automation-document-v1.json`](examples/automation-document-v1.json)
- Normalized plan schema: [`automation.plan.v1`](schemas/automation-plan.v1.schema.json)
- Resolved plan example: [`automation-plan-v1.json`](examples/automation-plan-v1.json)
- Architecture: [Agent-Authored Automation Engine](../superpowers/specs/2026-07-11-agent-authored-automation-engine-design.md)
- Compatibility decision: [ADR-0017](../adr/0017-version-automation-documents-and-plans.md)

An agent writes only the authored document. It never writes normalized plans,
run state, trace records, adapter methods, or executable code. HomeMagic parses,
resolves, type-checks, bounds, canonicalizes, and compiles the document before it
can be simulated or activated.

## Compatibility

The exact schema identifier is required. Unknown document or plan versions are
rejected rather than interpreted approximately. Existing immutable content is
never migrated in place; a pure migration creates a new automation version with
new validation and simulation evidence.

Canonical hashes cover the complete serialized contract. Struct field order is
stable and authored maps use ordered keys, so equivalent canonical documents
produce the same digest on macOS ARM and Linux x64.

## Safety and bounds

The IR supports only typed scalar expressions and bounded declarative control
flow. It has no script, template, loop, recursion, arbitrary JSON expression,
raw adapter operation, or vendor command dictionary.

Absolute bounds cap document bytes, plan nodes, nesting, branch width, queues,
retries, timers, run duration, and trace size. A document exceeding a bound fails
validation with an exact JSON Pointer; it is never silently truncated.

## Desired-state semantics

Commands targeting the same endpoint/capability inside one uninterrupted segment
reduce to the last desired state. A delay, wait, condition, external event, or
completed dispatch creates a boundary and preserves intentional intermediate
states.

The normalized `automation.plan.v1` contract is emitted only by the compiler and
contains resolved stable targets, deterministic node order, Safety Profiles,
approval requirement, reduction segments, and enforced budgets. It also carries
resolved triggers, the run-level guard, typed variable declarations, run mode,
and self-trigger policy so simulation and runtime never reopen authored aliases
or spaces.
