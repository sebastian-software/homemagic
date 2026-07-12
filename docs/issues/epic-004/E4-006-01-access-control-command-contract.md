---
id: E4-006-01
epic: EPIC-004
parent: E4-006
title: Define governed access-control command contracts
status: ready
priority: critical
depends_on: [E4-005]
adrs: [ADR-0014, ADR-0015, ADR-0035, ADR-0036]
created: 2026-07-12
updated: 2026-07-12
---

# E4-006-01: Access-Control Command Contract

## Outcome

`access_control.v1` has typed `lock` and `unlock` commands, security risk, exact
target policy semantics, and an explicit user-only approval authority without a
vendor or Matter payload escape hatch.

## Tasks

- [ ] Add typed `AccessControlCommand::Lock` and `Unlock` payloads.
- [ ] Treat both actions as replaceable desired state while preserving unlock's
  additional authorization requirement.
- [ ] Add a persisted actor principal kind with backward-compatible decoding.
- [ ] Add `approve_unlock` as an independently grantable action.
- [ ] Require exact capability scope and security risk for approval authority.
- [ ] Extend persisted round-trip, policy, and public command-schema tests.

## Acceptance criteria

- [ ] Invalid schema/payload combinations fail before policy or dispatch.
- [ ] Broad installation, device, or space grants cannot approve unlock.
- [ ] Agent, automation, and service actors cannot approve unlock even with a
  forged or accidentally broad grant.
- [ ] Existing persisted actors decode as user actors only where the historical
  bootstrap semantics prove that classification.

## Verification

- [ ] Domain state-machine and serialization tests pass.
- [ ] Policy matrix covers every actor kind and grant scope.
- [ ] No public request accepts Matter cluster, command, or attribute IDs.

