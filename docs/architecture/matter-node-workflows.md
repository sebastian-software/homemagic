# Matter Node Workflows

## Commissioning admission

`MatterNodeWorkflowService` is the application-owned boundary for Track A node
lifecycle work. It composes authenticated administration, durable Matter state,
and the SDK-neutral controller port. The current slice accepts only the
deterministic simulator and therefore proves application semantics, not Matter
protocol interoperability or physical-device compatibility.

Commissioning starts without setup bytes. `start_commission` reloads the actor,
requires the exact installation-scoped `matter_commission_node` grant, derives
the installation's stable fabric ID, verifies durable active fabric metadata,
and commits an actor-bound `requested` operation. Only after that operation is
returned may a caller construct `MatterCommissioningInput` for execution.

The sensitive input is non-serializable and its `Debug` representation is
redacted. Consuming it creates `MatterCommissioningRequest` directly at the
controller boundary. Setup bytes are not part of the operation target,
idempotency digest, SQLite data, events, logs, or ordinary diagnostics.

## Target semantics

ADR-0040 makes the operation target match facts known at admission time:

- commissioning targets `Fabric` because the authoritative node ID does not yet
  exist;
- cancellation targets `Operation` because it acts on an existing commissioning
  attempt;
- removal targets `Node` only after the controller returned its operational ID.

Operation targets remain immutable. A successful commissioning result will use
the schema 10 operation-to-node relation rather than rewriting historical
request facts.

## Durable result boundary

Schema 10 adds `matter_operation_node_results`. Each row links exactly
one commissioning operation to a stored fabric-scoped node and stable common
device. Foreign keys require the operation, node, and device to exist. The
repository exposes typed read and atomic write contracts so the node,
projections, subscription, result, and terminal operation progress become
visible together.

## Commissioning execution

`run_commission` reloads operation ownership and current authority, persists
`validating_setup`, and only then consumes `MatterCommissioningInput`. The
controller's bounded event page must contain the exact declared commissioning
phase sequence for that operation. Missing, duplicate, or reordered phases
become structured repair-required evidence instead of being inferred.

After the controller returns an authoritative descriptor, HomeMagic applies the
same versioned projection rules used everywhere else, performs one bounded read
for the selected scalar paths, and establishes one stable logical subscription.
The read supplies real initial on/off or lock state; no default state is
invented for the common device snapshot.

One repository transaction then writes or updates the stable Matter integration
and enrolled common device, inserts the node descriptor, capability projections,
established subscription, immutable operation-to-node result, and completed
operation progress. A failure at any point rolls back every newly visible node
fact. A second attempt to commission an already-present simulator node ends as a
structured conflict and cannot duplicate common identities.

## Cancellation and restart recovery

Cancellation always names an owned commissioning operation. While commissioning
is still `requested`, HomeMagic transitions it directly to `cancelled` and does
not call the controller. Once work has crossed the dispatch boundary,
`start_cancel_commissioning` first persists a separate actor-bound
`CancelCommissioning` operation targeted at the original operation. Its runner
then records `cancelling` before invoking the controller.

The controller's three normalized outcomes have deliberately different durable
meanings:

- `cancelled` commits the original as `cancelled` and the cancellation as
  `completed`;
- `already_completed` never claims reversal: the original becomes
  `repair_required` while the cancellation records a completed best-effort
  request;
- `outcome_unknown` makes both histories `repair_required`.

Both operation transitions, their immutable progress facts, and any repair
records share one SQLite transaction. A failed commit therefore leaves both
prior phases intact instead of creating contradictory histories. A cancellation
left durably in `cancelling` can be retried after reopen; this repeats only the
idempotent best-effort cancellation request, never commissioning. Ownership is
checked before disclosure, so an operation owned by another actor is returned
through the same not-found path as an absent operation.

`recover_commissioning` never accepts or reconstructs setup input and never
calls `commission`. A completed atomic operation returns its stored node result;
an already terminal operation is returned unchanged. For an interrupted
nonterminal operation it inspects only bounded controller progress and inventory.
Those sources currently lack an operation-to-node correlation fact, so they
cannot prove which operation created a visible node. Recovery therefore fails
closed to `repair_required` rather than guessing. This is intentional until the
controller contract can carry authoritative correlation evidence.

## Durable node inventory

`MatterNodeInventoryService` is the authenticated read boundary over durable
Matter node state. Every request reloads the actor and its current exact
installation-scoped `matter_read` grant. Repository queries bind both
installation and fabric, so a foreign fabric or node follows the same empty or
missing path as an absent one.

List pages accept 1 through 256 items and order nodes by operational node ID.
Summary DTOs contain stable fabric, node, common-device, projection,
subscription, descriptor-revision, and commissioning-operation identities.
Detail DTOs add the latest bounded SDK-neutral descriptor, projection metadata,
and logical subscription metadata. They contain no fabric secret
references, setup payloads, raw controller objects, or SDK types.

The repository loads each node and its relations from one read transaction.
Projection ordering is stable by endpoint, capability schema, and projection
identity. Inventory therefore remains byte-equivalent after reopen while newer
descriptor revisions replace only the durable descriptor payload and revision.

## Node removal

Removal admission reloads the actor's exact installation-scoped
`matter_remove_node` grant and requires an active durable node in that
installation. The immutable operation target contains the authoritative fabric
and node IDs. Actor-scoped idempotency returns the existing equivalent operation
for the same retry key and rejects reuse against another node.

Execution persists `removing_node` before the controller call. `removed` and
`not_present` have the same safe durable meaning: the controller no longer owns
an active node. `partial_outcome`, a still-present node after an error, or
unbounded controller ambiguity becomes structured `repair_required` evidence
while retaining every projection and subscription needed for diagnosis.

Once absence is proven, HomeMagic persists `cleaning_secrets` before local
cleanup. Nodes currently own no secret references separate from their fabric,
so fabric secrets are deliberately untouched. One SQLite transaction removes
the active projections and logical subscription, clears the common device's
capabilities, marks its lifecycle `removed` and availability `offline`, appends
completed operation progress, and retains the node identity plus commissioning
link as a tombstone. A failed transaction rolls every cleanup fact back.

Recovery never blindly repeats physical removal. From `removing_node`, one
bounded controller lookup may prove absence and allow local cleanup; presence or
unknown evidence requires repair. From `cleaning_secrets`, only the atomic local
cleanup is resumed. Replaying a completed operation returns immediately without
calling the controller.

## Read-only diagnostics

`MatterDiagnosticsService` implements ADR-0041 as a separate read-only
application boundary. It reloads the actor and exact installation-scoped
`matter_read` grant on every request, accepts only page sizes from 1 through
256, and has no controller mutation dependency. Its only live call is one
bounded `fabric_status` read.

The `matter.diagnostics.v1` document combines secret-free durable fabric health,
normalized controller availability and node count, common-device-keyed node and
endpoint counts, capability schema names, subscription freshness and explicit
repair eligibility, newest actor-owned operation phases, and an aggregate open
repair count. Operational node IDs, protocol endpoint IDs, operation targets,
network material, setup input, secret references, controller implementation
names, and raw SDK objects are intentionally absent.

Freshness is evaluated at an explicit caller-supplied time. Each logical
subscription persists its gap reason, sleepy-device flag, consumed and maximum
gap-read and subscribe-attempt budgets, retry deadline, last gap-read time, and
sleepy-read interval in the same versioned payload. Historical payloads receive
finite defaults during decoding; new writes reject zero or over-consumed
budgets.

The pure status projection derives `established`, `stale`, `waiting`,
`exhausted`, or `repair_required` plus an adapter-independent remediation code.
Retry and sleepy-read timestamps use exact inclusive boundaries: before a
deadline the status waits, while at the deadline the bounded action is eligible.
An ordinary stale subscription recommends a gap read or resubscribe; explicit
repair becomes eligible only after fixed-budget exhaustion or a durable
`repair_required` state. Reads never reset those facts, and reopening the
repository or recovery machine restores the same counters and deadline.
Diagnostics never start repair implicitly; the explicit repair children of
E4-007-04 own the separate mutation boundary.

## Explicit subscription repair

`MatterSubscriptionRepairService` is the separate mutation boundary required
by ADR-0041. Admission reloads the actor and exact installation-scoped
`matter_repair_subscription` grant, then requires an owned durable node and
logical subscription before creating an actor-bound idempotent
`RepairSubscription` operation. Reusing the same key and target returns the
original operation; another actor follows the same missing-operation path.

The requested operation is durable before projections or controller state can
change. Execution advances through `reading_gap` and `subscribing`. Entering
`reading_gap` atomically marks all node projections stale, retains
`repair_required` visibility on the subscription, resets only the explicit
repair's fixed policy budget, and appends operation progress. A failed
transaction restores every prior projection, subscription, and operation fact.

The gap read uses descriptor-derived, duplicate-free bounded report paths and
the normal report normalizer with `refresh_fallback` provenance. New reports
advance projection state with ordinary data-version and sequence rules;
duplicates may re-establish freshness, while stale or conflicting reports never
overwrite newer durable evidence. A failed or incomplete read leaves affected
projections visibly stale with `read_failed`, but the one bounded resubscribe
path may still restore notifications. No cluster write or unrelated command is
available to the workflow.

Before each subscribe call, the durable subscription reserves one unit of its
fixed attempt budget. A failure below the bound persists a deterministic retry
deadline and returns `waiting` without controller I/O before that deadline. At
the exact deadline the next reserved attempt may run. Success atomically marks
the stable logical subscription established and completes the operation;
exhaustion atomically retains `repair_required`, completes the attempt budget,
appends terminal operation progress, and opens structured repair evidence.
Contract call counting proves one gap read and no more subscribe calls than the
declared policy.

### Repair restart reconciliation

Both controller calls now have a durable pre-dispatch reservation. Entering
`reading_gap` consumes the one fixed gap-read unit before the read. Entering or
retrying `subscribing` consumes one subscribe-attempt unit before the subscribe
call. A crash can therefore never reset a counter and obtain another call for
free.

The current controller contract has no bounded query that can prove the outcome
of an interrupted gap read or subscribe dispatch. Reopening `reading_gap`, or
reopening `subscribing` without a persisted controller failure and retry
deadline, consequently transitions atomically to `repair_required` with an
`outcome_indeterminate` repair record. It never blindly repeats controller I/O.
A known failed subscribe is different: its durable future `retry_at` continues
to return `waiting` without I/O, and at the exact deadline the next attempt is
reserved and dispatched once.

Requested operations may resume because no controller barrier has been crossed.
Completed and repair-required operations replay their terminal result without
I/O. Unresolved repair rows protect their terminal operation from both age- and
count-based retention, so exhaustion and remediation survive reopen until an
explicit later repair resolves them. None of these restart paths authorizes
automation catch-up or replays unrelated commands.

## Verification

SQLite contracts cover allowed, denied, duplicate, conflicting-key,
inactive-fabric, light and lock projection, actual initial state, subscription,
atomic rollback, reopen, setup-canary, owner isolation, local and in-flight
cancellation, all cancellation outcomes, dual-history rollback, and all six
commissioning restart checkpoints. Unit contracts reject skipped, reordered,
and duplicate controller phases. Subscription contracts cover fresh, stale,
sleepy waiting, retry waiting, gap-budget completion, exhausted,
repair-required, exact-deadline, checkpoint, and reopen behavior. Inventory
contracts cover empty, populated,
bounded, foreign, disabled-actor, secret-canary, operation-link, and reopen
behavior. Historical migration fixtures cover schema 9 to schema 10. Full
workspace tests, all-feature strict Clippy, Matter dependency boundaries, and
the repository secret scan remain required before each committed child slice
closes.
