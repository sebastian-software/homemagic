# Matter Storage Boundary

## Purpose

HomeMagic persists Matter controller state through the application-owned
`MatterRepository` port. SQLite is one adapter for that port. Controller SDKs
must not own the durable schema, stable identities, command convergence, unlock
authorization, or restart policy.

This boundary implements ADR-0035 through ADR-0037. It does not select a Matter
controller SDK and does not prove protocol or hardware compatibility.

## Durable ownership

Migration `0006_matter_controller.sql` stores:

- one HomeMagic fabric row per stable `MatterFabricId`;
- fabric-scoped nodes keyed by `(fabric_id, node_id)`;
- descriptor endpoints keyed by stable protocol endpoint number;
- common-capability projections with desired and reported state;
- logical subscriptions without ephemeral SDK subscription identifiers;
- controller operations plus immutable progress facts;
- structured repair records;
- short-lived unlock decision facts without bearer material;
- one latest desired-command slot per projection; and
- immutable command-supersession relations.

Network addresses, discovery candidates, secure-session identifiers, and SDK
handles are deliberately absent from identity constraints. Address or session
changes therefore cannot create another node or endpoint identity.

## Transaction invariants

Every mutable aggregate uses an explicit optimistic revision. A create expects
no durable revision and writes revision one. An update supplies the exact
current revision and writes its successor. Conflicts fail without mutating the
stored row.

The following facts are indivisible SQLite transactions:

- operation phase, operation aggregate, progress fact, and optional matching
  repair record;
- old pre-dispatch command cancellation, cancellation audit, replacement slot,
  and supersession relation;
- validated-to-dispatched command transition, dispatch audit, and desired-slot
  dispatch marker; and
- unlock authorization consumption.

Failed validation, foreign-key violations, audit mismatches, and optimistic
conflicts roll the entire transaction back. A restart can therefore distinguish
undispatched, superseded, dispatched, nonterminal, and repair-required work from
durable facts alone.

## Recovery

`MatterRepository::recover_matter` returns bounded, deterministic pages of:

- every nonterminal operation ordered oldest first;
- pending, stale, or repair-required logical subscriptions;
- desired/reported projections that have not converged; and
- unresolved repairs.

The query accepts an explicit evaluation time. No automatic catch-up or hidden
physical action is implied: later application workflows decide whether to
resume, reconcile, ask the user, or expose repair guidance.

## Unlock authorization

An unlock authorization stores immutable identities and decision facts:
authorization, command, requesting actor, approving actor, projection, desired
revision, policy revision, issue time, expiry, and optional consumption time.
It stores no bearer credential. Consumption checks the exact command and
projection binding, rejects expiry, and changes `consumed_at` only once inside a
transaction. Lock commands do not need this unlock-specific authorization row.

## Secret boundary

The fabric payload contains only three opaque `SecretRef` values. Setup codes,
private keys, operational credentials, decrypted controller state, export keys,
and protected export envelopes are not accepted by the repository contract.
They remain behind `SecretStore` and controller sensitive-value boundaries.

Online backups copy the same reference-only rows. Storage diagnostics report
schema, integrity, and WAL state without fabric payloads. Contract tests inspect
the live database, a consistent backup, diagnostics, and failure paths with
secret canaries.

## Retention

Retention may remove bounded terminal operation history, resolved repairs, and
consumed or expired authorization facts. It cannot remove:

- nonterminal operations;
- current fabrics, nodes, endpoints, projections, subscriptions, or desired
  command slots;
- unresolved repairs; or
- unexpired authorization facts at the explicit retention evaluation time.

Foreign keys preserve referential integrity while deleting eligible history.

## Verification

`crates/homemagic-storage/tests/matter_repository_contract.rs` covers fresh
storage, reopen, all nonterminal operation phases, pending projection and
subscription recovery, optimistic conflicts, transaction rollback,
supersession, dispatch markers, concurrent single-use authorization, expiry,
malformed payloads, retention protection, database and backup secret canaries,
and stable identity.

`crates/homemagic-storage/tests/migration_fixtures.rs` covers empty and historical
schemas including an explicit schema-5-to-schema-6 upgrade. The workspace also
runs strict Clippy, all-feature tests, warning-denied Rustdoc, the Matter
dependency boundary check, and the repository secret scan.
