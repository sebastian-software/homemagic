---
id: E4-007-02-01
epic: EPIC-004
parent: E4-007-02
title: Stage and create the installation Matter fabric idempotently
status: done
priority: high
depends_on: [E4-007-01]
adrs: [ADR-0033, ADR-0037]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-02-01: Fabric Status and Create

## Outcome

Each installation has one stable HomeMagic fabric identity. An authenticated
exact-grant request stages secret material and reference-only metadata, returns
the durable operation immediately, and can run controller creation without
losing idempotency or claiming success early.

## Tasks

- [x] Derive the one stable fabric identity for an installation.
- [x] Generate and store bounded root, operational, and controller-state secret
  material before attaching references.
- [x] Stage unavailable reference-only fabric metadata before operation
  admission.
- [x] Admit create through `MatterAdministrationService` and return `requested`.
- [x] Transition to `creating_fabric` before the controller call.
- [x] Persist active fabric status and completed progress only after controller
  success.
- [x] Normalize controller or secret-store failure with cleanup/repair evidence.
- [x] Expose authenticated status without revealing secret references.

## Acceptance criteria

- [x] Equivalent retries return the same fabric and operation.
- [x] Controller create runs at most once after durable admission.
- [x] SQLite contains only opaque secret references.
- [x] Crash or failure cannot produce an untracked active fabric claim.

## Verification

- [x] SQLite-backed request/run/reopen/idempotency/failure contracts pass.
- [x] Database and debug secret-canary scans remain clean.
- [x] Full local workspace gates pass.
- [x] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: Implemented deterministic installation fabric identity,
  restart-safe secret staging, idempotent create admission, controller
  reconciliation, redacted status, and failure/repair evidence. Targeted
  repository and workflow contracts and the full local workspace gate pass;
  commit, push, and public CI remain pending.
- 2026-07-12: Public CI run `29202622965` passed the Linux x86_64 quality job
  and simulator hashes on Linux x86_64 and macOS ARM64. This child issue is
  done.
