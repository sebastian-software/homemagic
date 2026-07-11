# ADR-0022: Persist automation command attempts explicitly

- Status: Accepted
- Date: 2026-07-12

## Context

CommandService already makes one actor-scoped request idempotent and records
dispatch before physical I/O. The automation run must additionally decide which
target and retry attempt is active after restart.

Inferring this from trace length or the last command IDs is ambiguous for
multi-target nodes, partial failures, delayed outcomes, nested branches, and
trace retention. Reissuing every target on a partial failure would also undo
the desired-state reduction guarantee and create unnecessary physical actions.

## Decision

An active command node persists one bounded command-attempt record inside its
run aggregate:

- owning plan node and zero-based attempt number;
- original resolved target indices selected for that attempt;
- corresponding durable command IDs;
- phase: awaiting outcome, backoff, or ready to dispatch;
- deterministic backoff-ready instant when applicable.

Idempotency keys contain run ID, node ID, original target index, and attempt.
The first attempt selects every compiled target. A retry selects only targets
whose durable command failure code is explicitly retryable. Confirmed targets
are never dispatched again.

Backoff starts from the latest durable command update time, not process time.
The timer and attempt phase are committed atomically. Consuming the timer
increments the attempt and checkpoints dispatch-ready state before calling
CommandService again.

The attempt record is cleared only when the command node advances or applies
its terminal failure policy. The append-only run command ID list remains the
complete audit summary.

## Consequences

- Restart never guesses an attempt number or target subset.
- Partial multi-target retries avoid re-commanding confirmed devices.
- Backoff timer identity remains stable across repeated recovery.
- Trace retention cannot affect runtime correctness.
- The run schema grows by one bounded optional record without a new table.
