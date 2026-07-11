# ADR-0027: Enforce run modes against evolving durable admission state

- Status: Accepted
- Date: 2026-07-12

## Context

A scheduler pass can observe several occurrences for one automation at the same
instant. Counting only runs loaded at the beginning of the pass lets every
occurrence see the same capacity. That can admit multiple `single` runs or
exceed a parallel bound before the next repository read.

Queued and restart modes also require distinct durable effects. A queue is not
parallel execution, and restart cannot merely admit a new run while leaving the
prior run and its timers eligible to continue.

## Decision

The scheduler maintains evolving per-automation-version admission state during
each bounded pass and updates it immediately after every decision.

- `single` accepts only when no non-terminal run exists; later same-pass
  occurrences are durably suppressed.
- `restart` atomically transitions every prior non-terminal run to cancelled,
  appends an outcome trace with reason `restart_mode`, and cancels its pending
  or ready timers before accepting the new occurrence.
- `queued` permits one active run, leaves additional occurrences scheduled in
  deterministic FIFO order up to the declared queue capacity, and suppresses
  excess occurrences. The oldest queued occurrence is admitted on the first
  pass after the active run becomes terminal.
- `parallel` accepts up to `maximum_parallel` non-terminal runs and suppresses
  excess same-pass occurrences.

Run identity remains derived from occurrence identity. Reprocessing accepted
occurrences loads the existing run instead of creating another intent.

## Consequences

- Capacity decisions include work admitted earlier in the same scheduler pass.
- A restart cannot leave a durable timer able to resume the cancelled run.
- Queue order follows the occurrence ordering from ADR-0026 and survives
  restart without a separate in-memory queue.
- Restart cancellation does not attempt to undo a command already durably
  dispatched; its later outcome remains causal evidence and cannot resume the
  cancelled run.
