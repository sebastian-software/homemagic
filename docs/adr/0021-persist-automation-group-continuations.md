# ADR-0021: Persist automation group continuations in run state

- Status: Accepted
- Date: 2026-07-11

## Context

An automation run currently stores one ready plan-node ID. That is sufficient
for sequences, conditions, commands, and timers, but not for nested parallel or
race groups. Process-local futures or task handles cannot be reconstructed after
a restart and would make simulator/runtime decisions diverge.

The compiler already emits bounded branch entry nodes and one explicit join per
group. Runtime therefore needs only the remaining branch order and group
completion rule; it does not need to persist an interpreter stack or Rust
execution object.

## Decision

Each run persists a bounded stack of group continuations. One continuation
contains:

- the group and join node IDs;
- parallel or race completion semantics;
- remaining branch entries in deterministic plan order;
- whether the current branch stopped through stop_branch;
- the compiler-validated maximum ready-branch bound.

Entering a group checkpoints its continuation before interpreting a branch.
Reaching the matching join advances or removes the top frame atomically with
the run revision. Nested groups push nested frames.

The initial runtime processes one ready branch per durable step in stable plan
order. This is within every compiled maximum_parallel bound and matches the
deterministic simulator. A later scheduler may execute multiple ready branches
concurrently only if each branch receives independent durable state and the
observable reduction/trace order remains identical.

No Tokio task, future, adapter handle, or process-local pointer is persisted.

## Consequences

- Parallel and race groups resume exactly after restart.
- Nested continuation state is inspectable and bounded by compiler depth/width.
- stop_branch can advance the owning group without terminating the run.
- Initial execution favors deterministic correctness over branch throughput.
- Any future true concurrency requires a superseding ADR and migration.
