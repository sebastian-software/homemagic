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

## Atomic interpreter steps

`AutomationStepWrite` commits one optimistic run revision together with its
contiguous trace batch and timer creates/transitions in one SQLite transaction.
Every trace step and timer must belong to the same run. A stale run revision,
invalid timer edge, non-contiguous trace, or cross-run payload rolls the complete
step back. The contract test deliberately fails trace sequencing after a valid
run transition and proves the run revision did not advance.

## Shared decision evaluator

Simulation and runtime use the same pure expression, comparison, boolean
short-circuit, observation lookup, and IANA-time-window evaluator. The host owns
only continuous-duration behavior: simulation advances virtual time, while the
runtime must represent elapsed time with durable timers. This prevents the two
hosts from quietly developing different branch or variable semantics while
preserving restart-safe runtime waiting.

## Durable node checkpoints

The first runtime executor advances at most one lifecycle or plan node per
call. Pending acceptance, variable assignment, branch selection, joins,
completion, delay creation, and delay consumption each increment the optimistic
run revision. A non-terminal run may retain its state while checkpointing
progress; terminal runs remain immutable. Delay creation stores the waiting run,
trace, and deterministic timer atomically. After restart, the scheduler readies
the timer and the executor consumes it atomically with the next run revision.

Parallel and race nodes use the bounded continuation stack defined by ADR-0021.
Entering a group persists its join, remaining branches, completion rule, and
compiler-owned width bound before the first branch runs. Each matching join
either starts the next branch, completes all branches, or selects the first
successful race branch. Nested groups push another frame. The initial executor
runs one ready branch at a time in plan order, which stays within every
maximum-parallel bound and matches simulator ordering.

## Durable condition waits

A wait node evaluates against one freshly loaded immutable foundation snapshot.
If false, its timeout timer and waiting run revision are committed together.
Each relevant durable event may schedule another bounded runtime step: a now
true condition atomically cancels the timer and advances, while a still-false
condition leaves the run untouched. Once the scheduler marks the timer ready,
the executor consumes it in the same transaction that applies the compiled
timeout failure policy. Workers never sleep or hold mutable device state.

Nested continuous state-duration conditions still require their own durable
stability timer and are rejected explicitly rather than approximated.

## Scoped timer recovery

ADR-0023 assigns every durable timer a semantic kind and includes that kind in
its deterministic identity scope. Delay, wait-timeout, command-retry, and
state-duration timers can therefore coexist on one run/node without ambiguous
recovery or equal-instant ID collisions. Storage prevents a timer transition
from changing its kind, and runtime lookup always matches both node and kind.

## Governed command crash window

Command nodes can be enabled only by attaching CommandService and its durable
actor projection. The runtime has no dispatcher dependency and submits every
physical target through that service. Its actor-scoped idempotency key is
derived from run ID, node ID, target order, and attempt. Its deadline is derived
from the durable run creation time and compiled run-duration budget, so retrying
after restart recreates the same canonical request.

CommandService stores the command before dispatch. If the process stops after
dispatch but before the automation run records the returned command ID, the
same runtime step receives ExistingEquivalent; it neither dispatches again nor
creates another command. The automation then atomically checkpoints the command
ID, command trace, and next run revision. Non-terminal command states put the
run into waiting and are read back by durable command ID.

ADR-0022 makes retry state explicit in the run aggregate. Each attempt records
its original target indices, durable command IDs, phase, and retry-ready
instant. A retry timer is derived from the latest durable failed-command update
plus compiled backoff, then committed atomically with the attempt. Timer
consumption checkpoints the incremented dispatch-ready attempt before calling
CommandService. Only targets with explicitly retryable terminal failure codes
are selected; already confirmed targets are never dispatched again. The
compiled maximum retry count is therefore independent of trace retention.
