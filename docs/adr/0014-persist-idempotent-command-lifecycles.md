# ADR-0014: Persist idempotent command lifecycles before dispatch

- Status: Accepted
- Date: 2026-07-11

## Context

Network retries and process termination can otherwise repeat physical actions or
hide whether a command reached a device. Requested state, transport
acknowledgement, and observed confirmation are different facts.

## Decision

Every command is persisted before adapter dispatch with a server-generated
command ID, authenticated actor, target, versioned capability, typed payload,
deadline, idempotency key, correlation, causation, canonical request hash, and
state-machine version.

The unique idempotency scope is `(actor_id, idempotency_key)`. An equivalent
canonical request returns the existing command and never dispatches again. A
different request with the same key is rejected as `idempotency_conflict`.

States are `received`, `validated`, `rejected`, `dispatched`, `acknowledged`,
`confirmed`, `failed`, `timed_out`, and `cancelled`. Each transition appends an
immutable audit record in the same transaction as the current command row.
Adapter acknowledgement never implies physical confirmation.

After restart, commands not yet dispatched may resume after validation. A command
recorded as dispatched is never blindly dispatched again; observation and a
bounded read decide confirmation, failure, timeout, or an explicit indeterminate
failure requiring operator review.

Terminal command outcomes are retained for 90 days and at most 250,000 rows per
installation. Security audit transitions are retained for 365 days and at most
1,000,000 rows. The earlier bound wins; active non-terminal commands are never
pruned.

## Consequences

- Retries are safe within retained history.
- Physical and distributed transactions are not conflated.
- Storage migrations and repository transactions become part of command safety.
