# Deterministic Automation Simulation

## Construction boundary

`AutomationSimulator::simulate` accepts one immutable
`AutomationSimulationFixture` and nothing else. The fixture contains a normalized
plan, virtual trigger/run context, typed initial state, ordered synthetic state
changes, and declared command outcomes. There is no constructor parameter for a
`CommandDispatcher`, integration adapter, network client, repository, or secret
store, so physical I/O is unavailable by construction.

The shared interpreter boundary is split into scheduler, immutable-state, and
command-evaluation ports. The simulator supplies private in-memory virtual
implementations. E3-006 supplies governed runtime implementations while retaining
the same normalized node and condition semantics.

## Determinism

- virtual time begins at the fixture's accepted occurrence instant;
- state changes at the same instant are ordered by stable resolved target and
  field;
- plan nodes use compiler-owned IDs and order;
- branch and equal-ready group work uses stable branch order;
- command outcomes are consumed in declared attempt order;
- trace IDs are derived from run ID plus contiguous sequence;
- trace details and variables use ordered maps;
- duration and trace-step budgets terminate malformed or excessive fixtures.

The committed
[`automation-simulation-v1.json`](../evidence/fixtures/automation-simulation-v1.json)
snapshot fixes the normalized trace order, virtual timestamps, retry outcomes,
variables, terminal status, and canonical trace hash. Repeated simulation also
compares the complete serialized trace byte-for-byte.

## Trigger and time semantics

Synthetic schedule, observation-change, device-event, and command-outcome inputs
must match a compiled trigger. Self-trigger and every run mode can suppress the
input before action interpretation. Expired schedule windows emit
`missed_skipped`; they never run automatically. Explicit catch-up is a distinct
fixture decision that creates a new simulation run.

Five-field cron schedules are interpreted in their declared IANA timezone and
enumerated as UTC instants. `chrono-tz` handles nonexistent and repeated local
times; the DST fixture proves the nonexistent Europe/Berlin spring-forward
02:30 occurrence is skipped.

## Interpreter coverage

The virtual interpreter covers conditions, variables, delay/wait timers,
timeouts, retries/backoff, explicit failure policies and fallbacks, branches,
parallel groups, first-success races, joins, desired command intents, and terminal
outcomes. Simulated command intents include the compiler-derived Safety Profiles,
approval requirement, typed payload, resolved targets, attempt, virtual instant,
and declared governed outcome.
