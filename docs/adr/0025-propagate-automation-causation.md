# ADR-0025: Propagate exact automation causation through commands

- Status: Accepted
- Date: 2026-07-12

## Context

An automation can react to command outcomes and device observations. A command
emitted by a run may therefore produce another matching event after that run
has already completed. Inferring self-causation from currently active runs or
timestamps would be nondeterministic and would fail across restart.

The durable event stream previously retained a correlation ID and direct event
cause, but not the immutable automation version and run that initiated a
command. Command-transition events also omitted their endpoint and capability,
which prevented precise command-outcome trigger matching.

## Decision

Automation runtime command requests carry an optional `AutomationCausation`
value containing the stable automation ID, immutable version, and durable run
ID. `CommandService` persists this value in the command envelope and copies it
to every command-transition event produced from committed audit state.

Command-transition events also retain the resolved endpoint and versioned
capability schema from the accepted command. The new persisted fields use
serde defaults so records written before this decision remain readable.

No adapter, RPC caller, reconciliation job, or manually submitted command may
invent automation causation. They leave it absent unless an automation runtime
is the actual caller.

## Consequences

- Self-trigger policy can identify the exact causing version after completion
  and restart without timing heuristics.
- Command-outcome triggers can match resolved device, endpoint, capability, and
  state.
- The causal chain remains inspectable from run to command audit to event.
- Device observations can carry this provenance later when an adapter can
  prove their relationship to a command; unrelated notifications remain
  unlabelled rather than receiving speculative provenance.
