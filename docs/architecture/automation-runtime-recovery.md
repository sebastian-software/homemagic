# Automation Runtime Recovery Keys

E3-006 derives durable work identities from source facts instead of process-local
randomness:

- occurrence ID = automation ID + immutable version + normalized source key;
- run ID = accepted occurrence ID;
- timer ID = run ID + plan node ID + absolute ready instant;
- trace ID = run ID + contiguous sequence.

Reprocessing the same event cursor, schedule instant, accepted occurrence, or
timer after restart therefore addresses the existing row. Repository create
operations remain idempotent only when the complete payload matches, so a key
collision cannot silently replace different work.

The runtime repository also exposes stable-order bounded queries for active
identity/version pairs and direct run/timer lookup. Startup recovery first loads
active immutable plans, then pending work, and confirms individual run/timer
state before attempting continuation. This is the base contract for the durable
step coordinator; it does not itself authorize or dispatch commands.

## Durable scheduler pass

`AutomationScheduler` materializes only schedules belonging to active immutable
versions. It uses the declared IANA timezone and deterministic source key, then
stores a `scheduled` occurrence before considering admission. A bounded tick:

1. moves expired pending timers to `ready`;
2. records expired occurrence windows as `missed_skipped` and never creates a
   run for them;
3. applies single/queued/parallel admission bounds (restart admission is
   intentionally handled by the step coordinator cancellation phase);
4. transitions an eligible occurrence to `accepted`;
5. creates or confirms the deterministic pending run intent before any node is
   interpreted.

Re-running the same time window finds the advanced occurrence and deterministic
run instead of recreating either. The SQLite-backed contract repeats a scheduler
window, verifies one run, then advances through a later missed window and proves
that no second run appears.
