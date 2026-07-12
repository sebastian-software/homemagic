# ADR-0036: Supersede undispatched state and converge after dispatch

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0014, ADR-0015, ADR-0017, EPIC-004, E4-001

## Context

Agents and automations can produce several state requests faster than a device
can apply or report them. Dispatching `on -> off -> on` mechanically may create
visible flicker even though only the final state matters. Conversely, once a
physical command has been dispatched, HomeMagic cannot claim to retract an
effect that may already have happened.

This behavior belongs above integrations so Shelly, Matter, RPC, MCP, and
automations do not develop different convergence rules.

## Decision

Capability schemas explicitly mark commands that set a replaceable desired
state. Toggle, pulse, scene, momentary, calibrated movement, and other commands
with meaningful intermediate effects are ineligible unless their schema defines
safe convergence semantics.

Eligible requests update one durable desired-state slot keyed by device endpoint
and capability. Every accepted request receives a monotonic desired revision and
its own ADR-0014 command/audit record.

Before dispatch, the command worker atomically compares the candidate revision
with the current slot:

- if it is current, normal policy and dispatch continue;
- if a newer revision exists, the older command becomes `cancelled` with stable
  reason `superseded_before_dispatch` and a link to the replacing command;
- no adapter invocation occurs for the superseded command.

There is no promised debounce interval. Requests collapse only while they have
not crossed the durable `dispatched` boundary. Automation compilation may reduce
known uninterrupted sequences earlier under ADR-0017, but runtime still applies
this rule.

After dispatch, a newer desired revision never rewrites history or cancels the
physical fact. Observation and bounded reads determine the in-flight outcome;
the coordinator then dispatches or confirms the latest revision as needed. A
dispatched or acknowledged command is never blindly replayed after restart.

Lock and unlock are state-setting actions but retain ADR-0035 authorization. A
new `lock` may supersede a still-undispatched `unlock` and invalidates its unused
authorization. A new `unlock` can dispatch only with authorization bound to that
new desired revision.

Events and queries expose desired revision, reported state, confirmed revision,
freshness, convergence, supersession, and indeterminate outcomes. They do not
claim that an already-dispatched intermediate physical state was invisible.

## Consequences

- Rapid undispatched lighting changes converge to the final state without
  unnecessary adapter calls.
- Every caller and integration shares the same behavior.
- Audit history remains complete even when no physical dispatch occurs.
- Physical systems remain honest: final convergence is guaranteed only within
  observed capability and policy limits, not retroactively.
- Capability authors must classify whether a command is safely replaceable.
