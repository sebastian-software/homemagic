---
id: E4-007-02-01
epic: EPIC-004
parent: E4-007-02
title: Stage and create the installation Matter fabric idempotently
status: ready
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

- [ ] Derive the one stable fabric identity for an installation.
- [ ] Generate and store bounded root, operational, and controller-state secret
  material before attaching references.
- [ ] Stage unavailable reference-only fabric metadata before operation
  admission.
- [ ] Admit create through `MatterAdministrationService` and return `requested`.
- [ ] Transition to `creating_fabric` before the controller call.
- [ ] Persist active fabric status and completed progress only after controller
  success.
- [ ] Normalize controller or secret-store failure with cleanup/repair evidence.
- [ ] Expose authenticated status without revealing secret references.

## Acceptance criteria

- [ ] Equivalent retries return the same fabric and operation.
- [ ] Controller create runs at most once after durable admission.
- [ ] SQLite contains only opaque secret references.
- [ ] Crash or failure cannot produce an untracked active fabric claim.

## Verification

- [ ] SQLite-backed request/run/reopen/idempotency/failure contracts pass.
- [ ] Database and debug secret-canary scans remain clean.
