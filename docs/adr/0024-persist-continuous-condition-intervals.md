# ADR-0024: Persist continuous-condition intervals

- Status: Accepted
- Date: 2026-07-12

## Context

A state-duration condition is true only when its nested condition remains true
for the complete declared interval. Comparing the current time to an earlier
observation is insufficient: restart, transient false observations, nested
boolean conditions, and multiple duration clauses must preserve exact progress.

Sleeping in a runtime worker would lose state on restart and make simulation
and runtime use different evaluation semantics.

## Decision

The run aggregate persists bounded continuous-condition intervals for its
current plan node. Each interval contains:

- canonical nested-condition plus duration hash;
- owning node and duration;
- absolute ready instant and scoped state-duration timer ID;
- pending or mature phase.

When the nested condition first evaluates true, runtime atomically creates the
scoped timer and pending interval. A ready timer is consumed atomically before
the interval becomes mature. A mature interval evaluates true without another
timer while its nested condition remains true.

If the nested condition evaluates false, runtime removes that hashed interval
and cancels its pending timer. Other valid mature intervals on the same complex
condition remain available. Leaving the owning plan node removes all of its
intervals.

The shared evaluator retains evaluation order. Runtime supplies durable
interval state while simulation supplies virtual time, so both hosts make the
same short-circuit decisions.

## Consequences

- Restart cannot shorten or skip a continuous interval.
- A transient false value resets only the affected condition contract.
- Multiple duration clauses can mature deterministically one after another.
- Wait timeout and state-duration timers safely coexist through ADR-0023.
- Runtime workers never sleep to implement automation time.
