# Agent-Authored Automation Engine Design

- Status: Approved design awaiting repository review
- Date: 2026-07-11
- Epic: EPIC-003
- Depends on: EPIC-002 command and policy contracts

## Purpose

HomeMagic needs an automation engine that an agent can author without generating
executable code or forcing a user to assemble workflows through a UI. The engine
must make authored behavior deterministic, reviewable, simulatable, governable,
restart-safe, and unable to bypass the shared command control plane.

This design defines the architecture, intermediate representation, lifecycle,
scheduling semantics, runtime execution, governance, persistence, API, retention,
and verification strategy for EPIC-003.

## Design principles

1. Authored documents are data, never executable code.
2. An authored document is validated and compiled before any execution.
3. Simulation and runtime use the same normalized plan and step interpreter.
4. Runtime physical actions use only the EPIC-002 `CommandService`.
5. Every decision and side effect is durable, ordered, and explainable.
6. Time, parallelism, queues, retries, and resource use are explicitly bounded.
7. Missed schedule occurrences never execute automatically.
8. Intermediate commands collapse to the final desired state until an observable
   boundary makes the sequence intentional.
9. Capability-specific safety profiles govern activation more accurately than a
   single broad mechanical category.

## Architecture

EPIC-003 extends the existing modular monolith rather than introducing a service
boundary:

- `homemagic-domain` owns immutable automation documents, versions, lifecycle
  states, normalized plans, run state, timers, and trace contracts.
- `homemagic-application` owns parsing, validation, reference resolution,
  compilation, simulation, approval evaluation, the step interpreter, and the
  runtime coordinator.
- `homemagic-storage` owns atomic SQLite persistence, migrations, retention, and
  restart queries.
- `homemagic-api` exposes authenticated `automations.*` and run RPC methods.
- `homemagic` composes the engine and owns bounded scheduler wakeups.

An authored document is never interpreted directly:

```text
draft
  -> validate and resolve
  -> canonical bounded execution plan
  -> deterministic simulation
  -> approval when required
  -> atomic activation
  -> durable step interpretation
  -> governed CommandService actions
```

Simulation replaces real time, observations, events, and command execution with
virtual ports. Runtime supplies the real clock, durable event cursor, current
state projection, and `CommandService`. Neither path receives an integration
adapter.

## Identity, versions, and lifecycle

An automation has a stable `AutomationId`. Every saved edit creates an immutable,
monotonically numbered `AutomationVersion`. Active content is never mutated.

The lifecycle is:

```text
draft -> validated -> simulated -> awaiting_approval | ready -> active
active -> disabled -> active
active | disabled -> retired
```

Validation, simulation, rejection, approval, activation, rollback, disablement,
and retirement are immutable lifecycle facts. Editing any validated or approved
content creates a new version with no inherited evidence.

Rollback atomically changes the active-version pointer to an earlier immutable
version. It does not copy or rewrite that version and preserves the complete
history of the replaced active version.

## Authored intermediate representation

Every document declares one supported schema version and contains:

- provenance: author Actor, optional agent identity, source request, and concise
  rationale;
- triggers: observation change, normalized device event, schedule, and command
  outcome;
- conditions: typed comparison, boolean composition, time window, and
  state-duration predicates;
- actions: common capability command, delay, wait for condition, typed variable
  assignment, sequence, conditional branch, bounded parallel group, and race
  group;
- run mode: `single`, `restart`, bounded `queued`, or bounded `parallel`;
- explicit timeout, retry, failure, and self-trigger suppression behavior.

The IR has no script, template language, arbitrary expression evaluation, loops,
recursion, raw adapter operation, or vendor command dictionary. Values are JSON
primitives with declared types and references. All expressions are pure and
side-effect-free.

## Validation and normalized execution plan

Validation is side-effect-free. It resolves names, aliases, spaces, devices,
endpoints, and capabilities to stable identities against an exact registry
revision. It rejects unknown schema versions, missing or ambiguous references,
incompatible capabilities, type mismatches, impossible branches, cycles, and
unbounded behavior.

Errors contain a stable code, exact JSON Pointer document path, concise reason,
optional remediation, and non-sensitive related reference.

Compilation produces a canonical `ExecutionPlan` containing:

- stable plan-node identities;
- deterministic traversal and tie-breaking order;
- resolved stable references and inferred types;
- aggregate safety profile and approval requirement;
- explicit resource budgets;
- precomputed desired-state reduction segments;
- the source document hash and registry revision.

Hard bounds cover document bytes, node count, nesting depth, parallel width,
queue length, timer duration, retry count, trace steps, and total run duration.
Plans exceeding a bound are invalid; the runtime never truncates them silently.

## Safety profiles and activation

`RiskClass` remains a coarse command-policy ceiling. Automation activation uses
versioned capability-specific Safety Profiles:

- `comfort`: ordinary reversible behavior such as lighting;
- `comfort_motion`: ordinary reversible motion such as a calibrated roller
  shutter with stop support;
- `access_control`: locks, door closers, and related access-changing behavior;
- `flow_control`: valves and other material or energy flow controls;
- `security`: other privacy- or security-sensitive capabilities.

Profiles include concrete constraints such as fresh state, calibration, stop
support, position availability, presence, or explicit approval.

Successfully validated and simulated `comfort` and constrained
`comfort_motion` versions may become `ready` when the author has activation
authority. `access_control`, critical `flow_control`, and `security` versions
remain `awaiting_approval` until a user explicitly approves that immutable
version. EPIC-003 does not require a separate second Actor or a four-eyes model.

Approval permits activation but never bypasses runtime command policy. Each
physical command is independently authorized by EPIC-002 at execution time.

## Desired-state reduction

The compiler partitions actions into uninterrupted evaluation segments. Within
one segment, commands for the same device endpoint and capability reduce to the
last desired state. For example:

```text
light on -> light off -> light on
```

emits exactly one `light on` command.

The following are observable boundaries and flush pending desired state:

- delay or wait;
- condition evaluation or branch decision;
- external event consumption;
- an already completed dispatch;
- a construct whose semantics explicitly observes an intermediate result.

Therefore `light on -> delay 5 seconds -> light off` remains two commands. The
reducer never retracts a command that crossed the durable dispatched boundary.

## Deterministic scheduling

Schedules carry an explicit IANA timezone. Nonexistent local times during a DST
forward transition are skipped. Repeated local times during a DST backward
transition run once at the earlier occurrence.

Every expected occurrence becomes a durable occurrence record. If HomeMagic was
offline or unable to accept it before its occurrence window ended, it becomes
`missed_skipped`. It is never automatically replayed after restart.

An agent or operator may deliberately request catch-up. Catch-up creates a new
run with a new identity, current policy evaluation, and explicit causation back
to the skipped occurrence. It is not a mutation or delayed execution of the old
occurrence.

Same-timestamp work is ordered by durable event cursor, then automation ID,
automation version, and plan-node ID.

## Durable runtime and concurrency

For every accepted trigger, the runtime persists a run intent containing the
immutable automation version, trigger or occurrence identity, event cursor,
correlation ID, and causation ID before interpreting steps.

Each interpreter iteration is small and durable:

1. Load the immutable plan, current run, variables, and immutable state snapshot.
2. Evaluate exactly one deterministic step or ready bounded group.
3. Persist trace output, variables, timers, and next program counter.
4. Flush reduced physical actions through `CommandService` when required.
5. Persist each command ID and its durable outcome before continuing.

Run modes are explicit:

- `single` records and suppresses triggers while a run is active;
- `restart` cancels only undispatched work in the prior run and starts the latest
  trigger;
- `queued` stores triggers in durable cursor order up to the document bound and
  records overflow suppression;
- `parallel` runs up to the document bound and records excess suppression.

Timers are durable absolute instants. After restart, future timers resume and
expired delay or wait timers become ready. Missed schedule occurrences remain
skipped. Commands that reached `dispatched` or `acknowledged` use EPIC-002
observation-only recovery and are never blindly resubmitted.

Self-trigger suppression matches automation version and correlation or causation
chain. It prevents feedback loops without hiding unrelated external changes.

## Simulation

Simulation consumes the normalized plan with virtual time, a supplied initial
state, synthetic or recorded events, and declared command outcomes. It cannot
construct or receive a real `CommandDispatcher`.

The simulator emits the same ordered trace-step contract as runtime, including
trigger matching, condition values, branch selection, reduction, command intent,
policy result, timer changes, suppression, and terminal outcome. Infrastructure
timestamps and generated identities are supplied deterministically by the
simulation fixture.

The same plan, registry revision, initial state, event history, clock sequence,
and declared command outcomes must serialize to byte-equivalent normalized
traces across repeated simulations.

## Failure semantics

Retries are never implicit. Every retry count, backoff, eligibility rule, and
terminal behavior is declared in the document and bounded by the plan.

A runtime failure follows the enclosing action's explicit failure policy. It may
terminate a branch, terminate the run, or select a declared fallback. It cannot
silently disappear. Every decision is appended to the trace.

One failed automation run is isolated from other runs, scheduler progress, and
device sessions. Invalid or stale plans cannot activate. Infrastructure errors
remain distinct from policy denial and physical command failure.

## Authenticated RPC surface

Authoring and query methods:

- `automations.create_draft`
- `automations.update_draft`
- `automations.get`
- `automations.list`
- `automations.versions`

Verification and governance methods:

- `automations.validate`
- `automations.simulate`
- `automations.approve`
- `automations.reject`
- `automations.activate`
- `automations.rollback`
- `automations.disable`
- `automations.retire`

Operational methods:

- `automations.runs`
- `automations.run.get`
- `automations.trace`
- `automations.run.cancel`
- `automations.runs.create_from_missed`

The authenticated Actor is never accepted from request parameters. Optimistic
draft revisions prevent two agents from overwriting one another. Activation and
rollback update the active pointer atomically. Every mutation has an immutable
audit fact and correlation chain.

## Persistence model

A forward-only SQLite migration stores:

- automation identity and current draft head;
- immutable version documents and normalized plans;
- validation, simulation, rejection, and approval evidence keyed by exact
  content hash;
- the atomic active-version pointer;
- trigger occurrences, durable queues, runs, timers, and variables;
- append-only ordered trace steps and lifecycle audit facts.

Documents and plans use canonical serialization and a stable digest. Activation
requires successful evidence for the same document hash, plan hash, and registry
revision. Draft updates cannot invalidate historical evidence.

## Retention

Automation retention is independent from device events and commands:

- automation identities and activated immutable versions remain until explicit
  retirement/export policy permits removal;
- never-activated drafts are bounded by age and per-automation count;
- run summaries outlive detailed traces;
- detailed traces and simulation fixtures use shorter configurable retention;
- active runs, pending timers, approval evidence, active/rollback versions, and
  versions referenced by retained runs cannot be deleted.

One bounded retention pass records what was removed and never affects active
execution.

## Verification strategy

EPIC-003 requires:

- schema fixtures for every IR construct and supported schema version;
- property tests for serialization round trips, canonicalization, lifecycle
  transitions, bounded-plan rejection, and desired-state reducer invariants;
- validator tests for missing, ambiguous, stale, and incompatible references
  with exact JSON Pointer paths;
- virtual-time tests for schedules, DST, delays, waits, retries, timeouts, missed
  occurrences, and same-timestamp ordering;
- stable snapshots for simulation and runtime traces;
- restart tests for timers, queues, interrupted commands, and skipped schedules;
- policy tests for comfort auto-readiness and explicit sensitive approval;
- end-to-end parity fixtures using identical plan and event history in simulation
  and runtime;
- construction tests proving simulation cannot receive an integration adapter;
- strict formatting, Clippy, workspace tests, secret scanning, macOS ARM evidence,
  and Linux x86_64 CI.

## Required ADRs

Implementation starts by accepting four ADRs:

1. automation IR compatibility and normalized-plan versioning;
2. deterministic time, scheduling, missed-occurrence, and restart semantics;
3. capability Safety Profiles, approval, and activation authority;
4. automation version, run, trace, and simulation retention.

These ADRs refine ADR-0004 without replacing its core decision that automations
are declarative governed documents.

## Issue decomposition boundary

After this design is approved in-repository, EPIC-003 will be decomposed into
dependency-ordered issue documents for:

1. decisions and ADRs;
2. IR, identities, lifecycle, and schema;
3. validator, resolver, compiler, safety profiles, and reducer;
4. SQLite persistence, retention, and restart queries;
5. virtual clock and deterministic simulator;
6. durable runtime interpreter and scheduler;
7. governance and authenticated RPC;
8. operator documentation and exit audit.

Each issue carries frontmatter status, dependencies, tasks, acceptance criteria,
and a progress log. An issue is marked done only after its linked implementation,
tests, documentation, and repository-wide gates pass.
