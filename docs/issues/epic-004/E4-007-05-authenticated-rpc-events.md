---
id: E4-007-05
epic: EPIC-004
parent: E4-007
title: Publish authenticated Matter RPC and operation events
status: ready
priority: high
depends_on: [E4-007-02, E4-007-03, E4-007-04]
adrs: [ADR-0003, ADR-0012, ADR-0013, ADR-0016, ADR-0035]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-05: Authenticated RPC and Events

## Outcome

Versioned `matter.*` JSON-RPC methods expose the application workflows with
server-derived actor context, stable errors, immediate operation envelopes, and
actor-filtered durable operation events while common commands remain the only
normal device-action API.

## Tasks

- [ ] Define versioned schemas and stable errors for fabric, operation, node,
  subscription, diagnostics, repair, and unlock-approval methods.
- [ ] Keep setup, export, and restore input on dedicated sensitive request paths.
- [ ] Derive actor context server-side for every method.
- [ ] Route unlock approval to `CommandService::approve_unlock` without exposing
  authorization identifiers.
- [ ] Return operation envelopes immediately for long-running mutations.
- [ ] Project operation transitions into actor-filtered durable cursor events.
- [ ] Document executable examples, cancellation, restart, repair, and sensitive
  input handling.

## Acceptance criteria

- [ ] Params cannot supply actor or policy context.
- [ ] Public schemas contain no raw cluster, attribute, or command escape hatch.
- [ ] Common `commands.execute` controls simulated lights and locks.
- [ ] Reconnected event clients receive only authorized operation transitions.

## Verification

- [ ] SQLite-backed JSON-RPC happy, invalid, conflict, denied, and restart
  matrices pass.
- [ ] Actor isolation and event-cursor reconnect contracts pass.
- [ ] API examples validate against executable schemas.

## Progress log

- 2026-07-12: E4-007-04 and all four diagnostics/repair children completed with
  public cross-platform CI. This issue is ready.
