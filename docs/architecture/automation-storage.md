# Automation Storage Contract

## Ownership and trust boundary

The application layer owns `AutomationRepository`; SQLite is one adapter. The
repository accepts typed domain/application records and never interprets agent
input, resolves device references, or dispatches commands.

Storage recomputes the canonical document and normalized-plan hashes before an
immutable version is inserted. Validation, simulation, approval, and activation
must reference the exact document hash, plan hash, and registry revision. An
active pointer changes in the same transaction that checks the identity's
optimistic revision and all required evidence.

## Durable aggregates

- `automation_identities` owns operational state and the atomic active-version
  pointer.
- `automation_drafts` is mutable only through optimistic revisions.
- `automation_versions` stores immutable documents/plans plus append-only or
  state-machine-governed validation and simulation evidence.
- `automation_approvals` stores immutable exact-hash user decisions.
- `automation_occurrences` is the durable trigger queue and records scheduled,
  accepted, suppressed, and missed/skipped outcomes.
- `automation_runs`, `automation_timers`, and `automation_trace` store interpreter
  progress. Runs use optimistic revisions; timers and occurrences use explicit
  domain state machines; trace sequence is contiguous and append-only per run.

Every create operation for occurrences, runs, and timers is idempotent only when
the stable ID and complete payload match. Reusing an ID for different work is a
typed storage error.

## Restart and retention

Recovery returns bounded, stable-order pages of scheduled/accepted occurrences,
non-terminal runs, and pending/ready timers. Durable IDs prevent duplicate work
when the runtime resumes the same records.

Automation retention is independent from device, event, and command retention.
It deletes dependents in trace → timer → run → occurrence order. Active versions,
all non-retired rollback candidates, and any version referenced by retained work
remain protected. Only retired, old, unreferenced versions are eligible for
operator-authorized pruning; their approval records are deleted in the same
transaction. Drafts have their own cutoff and all categories are bounded per
transaction.

## Schema evolution

Migration `0003_automation_engine.sql` is forward-only and checksum protected.
Migration `0004_automation_event_runtime.sql` adds the optimistic singleton
event-consumer checkpoint and cursor-aware deterministic recovery index.
The committed schema-v2 fixture proves an existing command-control-plane database
upgrades to the automation schema and reopens at schema version 3.
