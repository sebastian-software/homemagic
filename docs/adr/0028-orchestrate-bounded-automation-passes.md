# ADR-0028: Orchestrate automation as bounded durable passes

- Status: Accepted
- Date: 2026-07-12

## Context

The event consumer, scheduler, and interpreter were individually durable, but
the daemon did not invoke them. Calling only schedule admission would also miss
event occurrences, while interpreting one run until completion could starve
other automations and device-session work.

A run-local plan, evaluation, command, or budget failure must not terminate the
automation worker or prevent a sibling run from advancing.

## Decision

`AutomationEngine` coordinates one bounded pass in this order:

1. consume at most 1,000 durable normalized events using ADR-0026;
2. materialize the supplied schedule window and apply ADR-0027 admission;
3. reload at most 1,000 recoverable runs in durable order;
4. execute at most one interpreter step per recovered run.

Stage-wide repository failures stop the current pass before later stages. A
run-local interpreter failure is attached to that run ID in the pass result and
iteration continues with its siblings.

The daemon runs this engine in a dedicated 100 ms worker, separate from device
discovery and freshness reconciliation. Both workers share graceful shutdown.
The automation worker advances its in-process schedule boundary only after a
successful pass, so a transient stage failure retries the same idempotent
window. Startup begins at current time and therefore never invents automatic
offline catch-up.

The runtime receives `CommandService` and its repository explicitly. No engine
or worker code owns a device adapter or alternative dispatch path.

## Consequences

- Active event subscriptions and plan execution now run in the production
  daemon rather than existing only as library capabilities.
- Long or waiting runs cannot monopolize a pass; progress is round-based.
- One invalid or over-budget run is observable but cannot stop a healthy run.
- Device discovery, freshness, and session supervision remain independently
  scheduled.
- Explicit persisted schedule catch-up remains a separate operation; startup
  does not infer user intent from downtime.
