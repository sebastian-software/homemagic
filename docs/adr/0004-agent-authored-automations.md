# ADR-0004: Store automations as declarative, governed documents

- Status: Accepted
- Date: 2026-07-11

## Context

The desired interaction is to describe an outcome to an agent instead of
assembling triggers and actions in a UI. Directly executing agent-generated code
would be difficult to validate, explain, migrate, or secure.

Home automation also has different risk levels: a light is not equivalent to a
door lock, camera, or moving cover.

## Decision

Agents create versioned automation documents in a typed intermediate
representation. Each version contains triggers, conditions, actions, concurrency
policy, timeouts, failure behavior, provenance, and a human-readable rationale.

The lifecycle is:

`draft -> validated -> simulated -> approved when required -> active -> retired`

Validation resolves capability references and checks types and policy. Simulation
uses synthetic or recorded events without dispatching real commands. Activation
is governed by risk policy:

- low-risk comfort actions may be auto-activated;
- mechanical actions require configured safety constraints;
- locks, cameras, and other security-sensitive actions require explicit approval
  by default.

Every execution carries the automation version and causation chain into the
command and audit records.

## Consequences

- Automations are diffable, testable, explainable, and reversible.
- Agents do not receive an unbounded code execution path.
- A later UI becomes another editor and renderer of the same documents.
- The intermediate representation and simulator become important product APIs.

