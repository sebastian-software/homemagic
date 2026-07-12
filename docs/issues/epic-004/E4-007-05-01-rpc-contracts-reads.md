---
id: E4-007-05-01
epic: EPIC-004
parent: E4-007-05
title: Publish Matter RPC contracts and authenticated reads
status: ready
priority: high
depends_on: [E4-007-04]
adrs: [ADR-0003, ADR-0013, ADR-0016, ADR-0041, ADR-0042]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-05-01: RPC Contracts and Reads

## Outcome

The router composes Matter services explicitly and exposes versioned,
authenticated, bounded, redacted fabric, operation, node, subscription, and
diagnostic read methods with stable transport errors.

## Tasks

- [ ] Add a Matter service bundle to API state without global singletons.
- [ ] Define `matter.*.v1` params, result envelopes, and stable error mapping.
- [ ] Implement fabric, operation list/get, node list/get, and diagnostics reads.
- [ ] Reject actor, policy, raw cluster, attribute, command, and oversized params.
- [ ] Publish machine-readable JSON schemas for every read method.

## Acceptance criteria

- [ ] Actor context always comes from bearer authentication.
- [ ] Foreign operation/node reads are indistinguishable from missing.
- [ ] DTOs and schemas contain no setup, secret, SDK, or native network fields.

## Verification

- [ ] Happy, empty, invalid, denied, foreign, bounded, and reopen RPC tests pass.
- [ ] Serialized schemas pass secret and raw-mutation canaries.
