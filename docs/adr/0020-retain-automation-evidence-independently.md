# ADR-0020: Retain automation versions, runs, and traces independently

- Status: Accepted
- Date: 2026-07-11

## Context

Automation documents, approval evidence, run summaries, detailed traces, timers,
and device events have different operational and audit value. Reusing device or
command retention could delete rollback targets or pending execution state.

## Decision

Automation storage has its own bounded retention policy per installation:

- active and rollback-eligible immutable versions, their plans, validation,
  simulation, and approval evidence are protected;
- versions referenced by retained runs and all active runs, timers, queues, and
  occurrences are protected;
- never-activated draft versions default to 30 days and at most 20 per automation;
- detailed simulation results default to 30 days and at most 100 per version;
- detailed run traces default to 30 days and at most 1,000,000 steps;
- terminal run and occurrence summaries default to 180 days and at most 250,000;
- retired, unreferenced versions remain for at least 365 days before explicit
  operator-authorized pruning.

The earlier time/count bound applies only to eligible rows. One bounded retention
transaction removes dependent data in safe order and records aggregate removal
evidence. Retention never mutates active pointers or current execution.

Run summaries remain after detailed trace expiry and report that trace detail is
no longer retained. Export/backup occurs through the existing durable storage
operations before any policy change that shortens retention.

## Consequences

- Rollback, pending work, and approval evidence cannot disappear accidentally.
- Detailed traces remain bounded independently from long-lived summaries.
- Migration, query, recovery, and retention tests are required before activation.
