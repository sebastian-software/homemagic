# Matter Administration Boundary

## Purpose

`MatterAdministrationService` is the single application-owned admission
boundary for internal and future JSON-RPC Matter administration callers. It
does not expose SDK types or raw cluster operations and does not execute a
controller mutation itself. Workflow services admit durable work here before
crossing `MatterController`.

## Authentication and authorization

Callers provide an `Actor` derived from authentication, never an actor ID inside
request parameters. The service reloads the durable actor and grants at every
admission, read, cancellation, and failure transition. Disabled or missing
actors fail closed.

Matter administration actions are independently grantable:

- `matter_read`;
- `matter_create_fabric`;
- `matter_commission_node`;
- `matter_cancel_operation`;
- `matter_remove_node`;
- `matter_export_fabric`;
- `matter_restore_fabric`; and
- `matter_repair_subscription`.

An enabled grant must contain the exact action, use the actor's exact
installation scope, and permit security risk. Device, capability, space, and
other-installation grants do not authorize administration. The CLI command
`actor-grant-matter-administration` replaces only this administration action
family and requires each selected action explicitly.

## Durable admission and idempotency

`MatterAdministrationRequest` contains only operation kind, typed resource
target, and actor-scoped idempotency key. Kind and target family are validated
before storage. The service derives installation, action, actor, policy version,
operation ID, and canonical request hash.

Schema 8 stores an immutable `matter_operation_bindings` row beside the existing
operation aggregate. It binds operation, actor, installation, exact action,
idempotency key, canonical request hash, and policy version. The binding and
initial `requested` operation progress commit in one SQLite transaction.

Repeating the same actor key and canonical request returns the original
operation. Reusing the key for another kind or target returns the existing
operation ID as a conflict and creates no second operation. Actor-owned list
queries are bounded to 256 rows and ordered newest first. Cross-actor reads are
indistinguishable from missing operations.

## Cancellation, failures, and restart

E4-007-01 permits local cancellation only while a commissioned-node operation
is still `requested`, before controller work begins. Later cancellation that
must coordinate with a controller belongs to E4-007-03.

Controller errors contain only structured category, code, retryability,
resource, and repair action. The administration boundary records terminal
non-repairable failures as `failed`. `AfterRepair`, explicit repair actions, and
indeterminate outcomes transition to `repair_required` and create an atomic
repair record with matching evidence.

Restart reads the binding and operation from SQLite. It never reconstructs actor
authority from request parameters and never treats an unfinished operation as
permission to repeat controller work. Later workflow issues decide how each
phase resumes or requests repair.

## Verification

`matter_repository_contract.rs` proves exact-grant admission, kind/target
validation, equivalent retries, conflicting retries, actor isolation, bounded
listing, pre-controller cancellation, structured failed and repair-required
outcomes, repair evidence, and reopen durability. Migration fixtures prove the
schema-7-to-schema-8 upgrade, while the full workspace runs strict Clippy,
tests, doctests, Rustdoc, and public cross-platform simulator evidence.
