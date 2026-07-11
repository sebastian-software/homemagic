# EPIC-003: Agent-Authored Automation Engine

- Milestone: M3
- Status: In progress
- Depends on: EPIC-002
- Unlocks: EPIC-005

## Objective

Build a deterministic automation lifecycle that lets agents author declarative,
versioned behavior without generating executable code or bypassing command
policy.

## User outcome

A user can describe a desired behavior to an agent. HomeMagic stores the proposed
automation as a draft, validates every reference and type, simulates it against
recorded or synthetic events, explains the result, obtains approval when policy
requires it, and activates an immutable version.

## Scope

- versioned automation intermediate representation (IR);
- typed triggers, conditions, actions, variables, timing, and concurrency modes;
- static validation and reference resolution;
- deterministic simulation with virtual time;
- execution engine with durable runs and traces;
- draft, version, approval, activation, rollback, and retirement lifecycle;
- risk aggregation and policy integration;
- RPC methods for the complete lifecycle;
- recorded-event fixtures and operational observability.

## Non-goals

- arbitrary Rust, JavaScript, Python, or template-code execution;
- natural-language parsing inside the engine;
- a visual workflow editor;
- distributed workflow orchestration;
- unbounded loops or recursion;
- direct integration-adapter access from automation actions.

## Finalized EPIC-002 contracts

- Every automation action submits a typed `CommandRequest` to the single
  `CommandService`; the engine cannot access an integration adapter.
- The authenticated automation actor, explicit grants, aggregate risk, current
  constraints, rate limit, and device concurrency participate in the same
  default-deny policy used by RPC.
- A run supplies stable idempotency, correlation, and causation identities and
  persists its own intent before submitting a command.
- Command acknowledgement and observation-confirmed outcome are separate; a run
  must not treat adapter acceptance as physical success.
- Restart recovery may inspect durable command/audit state but cannot blindly
  resubmit work that crossed the dispatched boundary.
- Simulation uses `commands.validate` semantics and has no real dispatcher by
  construction.

## Required decisions

- [ ] E3.D1: Finalize the automation IR and compatibility/versioning rules in an
  ADR that supersedes or refines ADR-0004 where necessary.
- [ ] E3.D2: Add an ADR for deterministic time, scheduling, and restart semantics.
- [ ] E3.D3: Add an ADR for automation approval, risk aggregation, and activation
  authority.
- [ ] E3.D4: Define trace and run retention separately from device telemetry.

## Workstream E3.1: Automation IR

- [ ] Define immutable automation ID and monotonically versioned documents.
- [ ] Define triggers for observation changes, events, schedules, and command
  outcomes.
- [ ] Define typed comparisons, boolean composition, temporal windows, and state
  duration conditions.
- [ ] Define actions for commands, delays, variable assignment, sequence,
  conditional branches, bounded parallel groups, and race groups.
- [ ] Define explicit single, restart, queued, and bounded-parallel run modes.
- [ ] Define timeout, retry, and failure behavior without hidden defaults.
- [ ] Include provenance, author/agent identity, rationale, and source request.
- [ ] Publish a machine-readable schema suitable for RPC and MCP clients.

## Workstream E3.2: Validation and resolution

- [ ] Parse and structurally validate documents without side effects.
- [ ] Resolve stable device, endpoint, capability, space, and alias references.
- [ ] Type-check trigger values, conditions, variables, and command payloads.
- [ ] Reject unknown schema versions and unsupported capabilities.
- [ ] Detect impossible branches, missing targets, cycles, and unbounded behavior.
- [ ] Calculate aggregate risk from every possible action path.
- [ ] Return errors with document paths and actionable remediation.
- [ ] Persist validation result against the exact document and registry revision.

## Workstream E3.3: Deterministic simulator

- [ ] Introduce a clock/scheduler port with real and virtual implementations.
- [ ] Simulate from recorded event streams without dispatching physical commands.
- [ ] Support synthetic initial state, events, and command outcomes.
- [ ] Produce ordered trigger, condition, branch, action, and policy trace steps.
- [ ] Show commands that would be submitted and their risk/policy decisions.
- [ ] Enforce deterministic ordering for equal timestamps.
- [ ] Compare simulation results through stable snapshots.
- [ ] Ensure simulation cannot invoke a real adapter by construction.

## Workstream E3.4: Runtime engine

- [ ] Subscribe only active automation versions to normalized events.
- [ ] Evaluate conditions from an immutable state snapshot.
- [ ] Submit actions only through the EPIC-002 command service.
- [ ] Persist run start, trace steps, pending timers, and terminal outcome.
- [ ] Restore or explicitly terminate interrupted runs according to documented
  semantics.
- [ ] Enforce concurrency mode, queue bound, timeout, and cancellation.
- [ ] Prevent self-trigger loops through causation and configurable suppression.
- [ ] Isolate one failing automation from other runs and device sessions.

## Workstream E3.5: Governance and RPC

- [ ] Add `automations.create_draft`.
- [ ] Add `automations.validate`.
- [ ] Add `automations.simulate`.
- [ ] Add `automations.approve` and `automations.reject`.
- [ ] Add `automations.activate`, `rollback`, `disable`, and `retire`.
- [ ] Add `automations.get`, `list`, `versions`, `runs`, and `trace`.
- [ ] Require approval for policy-selected mechanical or security actions.
- [ ] Make activation atomic and preserve the previous active version for rollback.
- [ ] Audit every lifecycle transition and execution causation chain.

## Test and verification checklist

- [ ] Schema fixtures cover every IR construct and supported version.
- [ ] Property tests cover parser/serializer round trips and bounded execution.
- [ ] Validator tests cover missing, stale, ambiguous, and incompatible references.
- [ ] Virtual-time tests cover schedules, delays, duration conditions, retries,
  timeouts, and same-timestamp ordering.
- [ ] Snapshot tests cover human-readable simulation and execution traces.
- [ ] Restart tests cover pending timers, queues, and interrupted commands.
- [ ] Policy tests cover low-risk auto-activation and required approval.
- [ ] End-to-end fixtures prove equivalent simulation and runtime decisions for
  the same event history.

## Acceptance criteria

- [ ] AC1: An automation document can be created, validated, simulated, approved,
  activated, rolled back, disabled, and retired solely through RPC.
- [ ] AC2: The same inputs and registry revision produce byte-equivalent normalized
  simulation traces across repeated runs.
- [ ] AC3: Invalid references and type errors prevent activation and identify their
  exact document path.
- [ ] AC4: Simulation has no code path to a physical integration adapter.
- [ ] AC5: Runtime actions pass through the same command, policy, idempotency, and
  audit services used by direct clients.
- [ ] AC6: Every run identifies the immutable automation version and complete
  causation chain.
- [ ] AC7: A previous active version can be restored atomically after a bad change.

## Exit gate

- [ ] All acceptance criteria contain linked evidence.
- [ ] Required ADRs are accepted and indexed.
- [ ] IR schema and examples are published with compatibility rules.
- [ ] No arbitrary-code or raw-adapter escape hatch exists.
- [ ] Operator documentation covers stuck runs, disable, rollback, and trace use.
- [ ] EPIC-005 references the finalized automation lifecycle and schemas.

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Agent produces plausible but unsafe behavior | Typed validation, simulation, risk policy, and approval |
| Time-dependent tests become flaky | Virtual clock and deterministic event ordering |
| Automation loops create command storms | Causation tracking, run modes, bounds, and suppression |
| IR grows into a programming language | Bounded constructs and ADR review for every extension |

## Progress log

- 2026-07-11: Epic created; blocked on EPIC-002.
- 2026-07-11: Approved the durable step-interpreter design with capability Safety
  Profiles, desired-state reduction, no automatic missed-schedule catch-up, and
  shared simulation/runtime semantics. Added the dependency-ordered E3 issue set;
  E3-001 is ready.
