# ADR-0018: Use deterministic automation time and never replay missed schedules

- Status: Accepted
- Date: 2026-07-11

## Context

Wall-clock scheduling, delays, process restarts, DST transitions, and equal-time
events otherwise make simulation and runtime disagree. Automatically replaying
actions missed while a home controller was offline would also be surprising and
potentially unsafe.

## Decision

The automation engine depends on application-owned clock and scheduler ports.
Runtime uses durable absolute instants; simulation uses virtual time. Both run
the same normalized step interpreter.

Schedules specify an IANA timezone. A nonexistent local time during DST is
skipped. A repeated local time runs once at the earlier occurrence. Every
expected occurrence receives a durable status.

An occurrence not accepted before its window ends becomes `missed_skipped` and
never executes automatically after restart. An agent or operator may explicitly
create a new catch-up run; that run has a new identity, current policy evaluation,
and causation back to the skipped occurrence.

Timers, queues, and run program counters commit before scheduler acknowledgement.
Future timers resume after restart and expired delay/wait timers become ready.
Commands already recorded as dispatched use EPIC-002 observation-only recovery.

Equal-time work is ordered by durable event cursor, automation ID, version, then
plan-node ID. Scheduler queues, parallelism, retries, timer duration, trace size,
and total run duration are bounded.

## Consequences

- Simulation traces are reproducible and runtime restart behavior is explicit.
- Offline time cannot cause a surprise burst of physical actions.
- Catch-up is an intentional, separately audited action rather than hidden replay.
