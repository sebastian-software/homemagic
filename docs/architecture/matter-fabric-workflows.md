# Matter Fabric Workflows

## Scope and evidence

`MatterFabricWorkflowService` composes the authenticated administration service,
durable Matter repository, `MatterController`, and `SecretStore` for the first
fabric lifecycle. The current workflow deliberately accepts only the
`homemagic-deterministic-simulator` implementation. Every returned status and
export carries `deterministic_simulator`; it is application-semantics evidence,
not Matter protocol, interoperability, certification, or hardware evidence.

Production restore format validation is separate and unconditionally rejects
`simulator_v1`. A simulator artifact cannot be relabelled as a `protected_v1`
production artifact by a caller.

## Identity and authorization

ADR-0037 allows one active HomeMagic fabric per installation. Its stable ID is
derived from the durable installation ID, so retries do not allocate new fabric
identities. Every status, create, export, and restore entry point reloads the
durable actor and exact installation-scoped administration action.

The workflow uses a start/run split:

1. `start_*` authenticates, canonicalizes the request, commits an actor-bound
   operation, and returns its envelope before controller work;
2. `run_*` reloads operation ownership and current authority, then crosses the
   controller port;
3. success persists fabric metadata and terminal progress;
4. structured controller failure becomes `failed` or `repair_required` through
   the shared administration service.

## Secret staging and fabric creation

Schema 9 adds `matter_fabric_stages`. Before fabric metadata attachment, SQLite
records the installation, deterministic fabric ID, requesting actor, three opaque
secret references, stage state, revision, and timestamp. It never stores secret
values.

The stage advances through `pending_secrets`, `secrets_ready`, or
`cleanup_required`. Secret values are random 256-bit values written through the
configured `SecretStore`. A failed write leaves restart-visible staging facts;
an explicit retry reuses the same references. Once all values exist, the
workflow attaches unavailable fabric metadata and only then removes the stage.
No automatic backend fallback or plaintext path exists.

`run_create` changes `requested` to `creating_fabric` before controller work.
If restart finds controller status proving the fabric exists, it activates the
durable row and completes without another create. If status cannot prove whether
work occurred, the operation becomes `repair_required`; restart is not treated
as permission to create blindly.

## Simulator export and restore

Export returns a non-serializable `MatterSimulatorExport`. Its `Debug` output
redacts both envelope and recovery key. Neither value enters the operation,
canonical request hash, SQLite, events, or ordinary diagnostics. Process loss
after controller export but before delivery becomes `repair_required`; the
workflow never regenerates a key under the old operation.

Restore admission occurs before sensitive bytes are accepted. The envelope and
key live only in non-serializable, redacted `MatterSimulatorRestoreInput`.
Invalid keys and corrupt envelopes become structured terminal operations without
retaining input. If restart finds live status proving restore completed, it
finishes without reusing lost input or issuing a second restore. A fresh restore
against an active controller fails with a durable conflict.

## Verification

SQLite contracts cover exact grants, stable identity, equivalent create retries,
secret-stage failure and retry, create restart reconciliation, lost export
output, valid export/restore, invalid key, corrupt envelope, restore restart,
redacted debug output, database/WAL secret canaries, and production rejection of
simulator formats. Migration fixtures cover schema 8 to schema 9.
