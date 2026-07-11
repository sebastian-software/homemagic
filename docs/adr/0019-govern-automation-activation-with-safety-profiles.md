# ADR-0019: Govern automation activation with capability Safety Profiles

- Status: Accepted
- Date: 2026-07-11

## Context

A single mechanical category conflates ordinary roller-shutter movement with
door closers, locks, and valves. Automation activation needs a user-comprehensible
rule without introducing a complex four-eyes organization model.

## Decision

Automation validation derives versioned capability Safety Profiles in addition
to EPIC-002 `RiskClass`:

- `comfort` for ordinary reversible state changes;
- `comfort_motion` for ordinary reversible motion with required constraints;
- `access_control` for access-changing actuators;
- `flow_control` for material or energy flow controls;
- `security` for other privacy- or security-sensitive behavior.

Profiles carry concrete constraints such as fresh state, calibration, stop
support, position availability, presence, or explicit approval.

Successfully validated and simulated `comfort` and constrained
`comfort_motion` versions may become ready when their authenticated author has
activation authority. `access_control`, critical `flow_control`, and `security`
versions require an explicit user approval for the exact immutable version.
EPIC-003 does not require approval by a different Actor.

Editing approved content creates a new unapproved version. Approval permits
activation but never bypasses runtime command authorization. Every physical
action still passes through `CommandService` with current actor grants, state,
constraints, rate limits, idempotency, and audit.

Within one uninterrupted evaluation segment, same-target commands reduce to the
last desired state. Delay, wait, condition, external event, or completed dispatch
boundaries preserve intentional intermediate states.

## Consequences

- Roller shutters can remain convenient without weakening lock or valve safety.
- The initial approval model stays understandable to a home operator.
- Safety Profile mapping and constraints require versioned tests as new
  capabilities are added.
